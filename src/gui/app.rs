//! GUI application state and logic

use crate::audio::{AudioEngine, EngineError, SoundId};
use crate::gui::types::{AppState, AudioCommand, ServerCommand, SoundInfo, ServerStatus as AppServerStatus};
use crate::gui::theme::SoundSharedColors;
use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError};
use egui::{Context, Vec2};
use rfd::FileDialog;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

/// Sound entry in the library (for GUI display)
#[derive(Debug, Clone)]
pub struct SoundEntry {
    pub id: SoundId,
    pub name: String,
    pub path: PathBuf,
    pub playing: bool,
    pub volume: f32,
}

/// Internal server status for GUI
#[derive(Debug, Clone, PartialEq)]
pub enum ServerStatus {
    Stopped,
    Starting,
    Running,
    Error(String),
}

/// Server state snapshot for GUI
#[derive(Debug, Clone)]
pub struct ServerState {
    pub status: ServerStatus,
    pub bind_url: Option<String>,
    pub listener_count: usize,
}

/// Main GUI application state
pub struct SoundSharedApp {
    // Reference to shared app state
    app_state: AppState,
    
    // Audio engine (must stay on main thread due to cpal::Stream not being Send)
    audio_engine: Arc<Mutex<Option<AudioEngine>>>,
    
    // Audio command receiver (for processing commands on main thread)
    audio_cmd_rx: Receiver<AudioCommand>,
    
    // Local sound cache (synced from audio engine)
    sounds: Arc<Mutex<HashMap<SoundId, SoundEntry>>>,
    sound_order: Arc<Mutex<Vec<SoundId>>>,
    
    // Master volume cache
    master_volume: Arc<Mutex<f32>>,
    
    // Server state cache
    server_state: Arc<Mutex<ServerState>>,
    
    // UI feedback
    copied_feedback: Arc<Mutex<bool>>,
    copied_feedback_timer: Arc<Mutex<Option<Instant>>>,
    
    // Pending file load
    pending_load: Arc<Mutex<Option<PathBuf>>>,
    
    // Channel for receiving sound list updates
    sound_list_rx: Receiver<Vec<SoundInfo>>,
    sound_list_tx: Sender<Vec<SoundInfo>>,
    
    // Channel for receiving server status updates
    server_status_rx: Receiver<AppServerStatus>,
    server_status_tx: Sender<AppServerStatus>,
}

impl SoundSharedApp {
    /// Create a new GUI app state with shared AppState and audio engine
    pub fn new(
        app_state: AppState,
        audio_engine: Arc<Mutex<Option<AudioEngine>>>,
        audio_cmd_rx: Receiver<AudioCommand>,
        _cc: &eframe::CreationContext<'_>,
    ) -> Self {
        // Create channels for receiving updates from background threads
        let (sound_list_tx, sound_list_rx) = bounded(1);
        let (server_status_tx, server_status_rx) = bounded(1);
        
        // Initialize master volume from config
        let initial_master_vol = app_state.config.lock().unwrap().audio.master_volume;
        
        let app = Self {
            app_state,
            audio_engine,
            audio_cmd_rx,
            sounds: Arc::new(Mutex::new(HashMap::new())),
            sound_order: Arc::new(Mutex::new(Vec::new())),
            master_volume: Arc::new(Mutex::new(initial_master_vol)),
            server_state: Arc::new(Mutex::new(ServerState {
                status: ServerStatus::Stopped,
                bind_url: None,
                listener_count: 0,
            })),
            copied_feedback: Arc::new(Mutex::new(false)),
            copied_feedback_timer: Arc::new(Mutex::new(None)),
            pending_load: Arc::new(Mutex::new(None)),
            sound_list_rx,
            sound_list_tx,
            server_status_rx,
            server_status_tx,
        };
        
        // Request initial sound list
        app.request_sound_list();
        
        // Request initial server status
        app.request_server_status();
        
        app
    }
    
    // ===== Public API =====
    
    /// Request loading a sound file via native file dialog
    pub fn request_load_sound(&self, path: PathBuf) {
        *self.pending_load.lock().unwrap() = Some(path.clone());
        
        let (response_tx, response_rx) = bounded(1);
        let cmd = AudioCommand::LoadSound { 
            path: path.to_string_lossy().to_string(), 
            response: response_tx 
        };
        
        if let Err(e) = self.app_state.audio_cmd_tx.try_send(cmd) {
            error!("Failed to send load sound command (channel full): {}", e);
            *self.pending_load.lock().unwrap() = None;
            return;
        }
        
        // Handle response in background
        let sounds = self.sounds.clone();
        let sound_order = self.sound_order.clone();
        let pending_load = self.pending_load.clone();
        let sound_list_tx = self.sound_list_tx.clone();
        
        std::thread::spawn(move || {
            match response_rx.recv() {
                Ok(Ok(_sound_info)) => {
                    // Note: SoundEntry was already inserted into `sounds` and `sound_order`
                    // by process_audio_commands() on the main thread. Do NOT insert again here
                    // to avoid duplicate entries in the GUI list.
                    *pending_load.lock().unwrap() = None;
                    info!("Loaded sound: {} ({})", _sound_info.name, _sound_info.id);
                }
                Ok(Err(e)) => {
                    error!("Failed to load sound: {}", e);
                    *pending_load.lock().unwrap() = None;
                }
                Err(_) => {
                    error!("Audio command channel closed");
                    *pending_load.lock().unwrap() = None;
                }
            }
        });
    }
    
    /// Request playing a sound
    pub fn request_play_sound(&self, id: SoundId) {
        let sounds = self.sounds.clone();
        let (response_tx, response_rx) = bounded(1);
        let cmd = AudioCommand::PlaySound { id, response: response_tx };
        
        if let Err(e) = self.app_state.audio_cmd_tx.try_send(cmd) {
            error!("Failed to send play sound command (channel full): {}", e);
            return;
        }
        
        std::thread::spawn(move || {
            if response_rx.recv().unwrap_or(Err(anyhow::anyhow!("Failed"))).is_ok() {
                if let Some(mut entry) = sounds.lock().unwrap().get_mut(&id) {
                    entry.playing = true;
                }
            }
        });
    }
    
    /// Request stopping a sound
    pub fn request_stop_sound(&self, id: SoundId) {
        let sounds = self.sounds.clone();
        let (response_tx, response_rx) = bounded(1);
        let cmd = AudioCommand::StopSound { id, response: response_tx };
        
        if let Err(e) = self.app_state.audio_cmd_tx.try_send(cmd) {
            error!("Failed to send stop sound command (channel full): {}", e);
            return;
        }
        
        std::thread::spawn(move || {
            if response_rx.recv().unwrap_or(Err(anyhow::anyhow!("Failed"))).is_ok() {
                if let Some(mut entry) = sounds.lock().unwrap().get_mut(&id) {
                    entry.playing = false;
                }
            }
        });
    }
    
    /// Request setting sound volume
    pub fn request_set_volume(&self, id: SoundId, volume: f32) {
        let sounds = self.sounds.clone();
        let volume = volume.clamp(0.0, 1.0);
        let (response_tx, response_rx) = bounded(1);
        let cmd = AudioCommand::SetVolume { id, volume, response: response_tx };
        
        if let Err(e) = self.app_state.audio_cmd_tx.try_send(cmd) {
            error!("Failed to send set volume command (channel full): {}", e);
            return;
        }
        
        std::thread::spawn(move || {
            if response_rx.recv().unwrap_or(Err(anyhow::anyhow!("Failed"))).is_ok() {
                if let Some(mut entry) = sounds.lock().unwrap().get_mut(&id) {
                    entry.volume = volume;
                }
            }
        });
    }
    
    /// Set master volume
    pub fn set_master_volume(&self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        *self.master_volume.lock().unwrap() = volume;
        
        let (response_tx, _response_rx) = bounded(1);
        let cmd = AudioCommand::SetMasterVolume { volume, response: response_tx };
        
        if let Err(e) = self.app_state.audio_cmd_tx.try_send(cmd) {
            error!("Failed to send set master volume command (channel full): {}", e);
        }
    }
    
    /// Get master volume
    pub fn get_master_volume(&self) -> f32 {
        *self.master_volume.lock().unwrap()
    }
    
    /// Request removing a sound
    pub fn request_remove_sound(&self, id: SoundId) {
        let sounds = self.sounds.clone();
        let sound_order = self.sound_order.clone();
        let (response_tx, response_rx) = bounded(1);
        let cmd = AudioCommand::RemoveSound { id, response: response_tx };
        
        if let Err(e) = self.app_state.audio_cmd_tx.try_send(cmd) {
            error!("Failed to send remove sound command (channel full): {}", e);
            return;
        }
        
        std::thread::spawn(move || {
            if response_rx.recv().unwrap_or(Err(anyhow::anyhow!("Failed"))).is_ok() {
                sounds.lock().unwrap().remove(&id);
                sound_order.lock().unwrap().retain(|&x| x != id);
            }
        });
    }
    
    /// Get all sounds for display
    pub fn get_sounds(&self) -> Vec<SoundEntry> {
        let sounds = self.sounds.lock().unwrap();
        let order = self.sound_order.lock().unwrap();
        order.iter()
            .filter_map(|id| sounds.get(id).cloned())
            .collect()
    }
    
    /// Request starting the server
    pub fn request_start_server(&self) {
        // Double-click protection: don't start if already starting or running
        {
            let state = self.server_state.lock().unwrap();
            if matches!(state.status, ServerStatus::Starting | ServerStatus::Running) {
                return;
            }
        }

        // Update local state to starting
        {
            let mut state = self.server_state.lock().unwrap();
            state.status = ServerStatus::Starting;
            state.bind_url = None;
            state.listener_count = 0;
        }

        let (response_tx, response_rx) = bounded(1);
        let cmd = ServerCommand::Start { response: response_tx };

        // Non-blocking send to avoid freezing GUI if channel is full
        if let Err(e) = self.app_state.server_cmd_tx.try_send(cmd) {
            error!("Failed to send start server command (channel full): {}", e);
            let mut state = self.server_state.lock().unwrap();
            state.status = ServerStatus::Error("Server command channel full".to_string());
            return;
        }

        let server_state = self.server_state.clone();
        let server_status_tx = self.server_status_tx.clone();

        std::thread::spawn(move || {
            // Wait for response with timeout to avoid hanging forever
            match response_rx.recv_timeout(Duration::from_secs(10)) {
                Ok(Ok(url)) => {
                    let mut state = server_state.lock().unwrap();
                    state.status = ServerStatus::Running;
                    state.bind_url = Some(url.clone());
                    info!("Server started successfully at {}", url);
                    let _ = server_status_tx.try_send(AppServerStatus {
                        running: true,
                        address: Some(url),
                        listener_count: state.listener_count,
                    });
                }
                Ok(Err(e)) => {
                    error!("Failed to start server: {}", e);
                    let mut state = server_state.lock().unwrap();
                    state.status = ServerStatus::Error(e.to_string());
                    let _ = server_status_tx.try_send(AppServerStatus {
                        running: false,
                        address: None,
                        listener_count: 0,
                    });
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    error!("Server start timed out after 10 seconds");
                    let mut state = server_state.lock().unwrap();
                    state.status = ServerStatus::Error("Server start timed out".to_string());
                    let _ = server_status_tx.try_send(AppServerStatus {
                        running: false,
                        address: None,
                        listener_count: 0,
                    });
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    error!("Server command channel closed");
                    let mut state = server_state.lock().unwrap();
                    state.status = ServerStatus::Error("Channel closed".to_string());
                }
            }
        });
    }
    
    /// Request stopping the server
    pub fn request_stop_server(&self) {
        let server_state = self.server_state.clone();
        let server_status_tx = self.server_status_tx.clone();
        
        let (response_tx, response_rx) = bounded(1);
        let cmd = ServerCommand::Stop { response: response_tx };
        
        // Non-blocking send to avoid freezing GUI if channel is full
        if let Err(e) = self.app_state.server_cmd_tx.try_send(cmd) {
            error!("Failed to send stop server command (channel full): {}", e);
            return;
        }
        
        std::thread::spawn(move || {
            match response_rx.recv_timeout(Duration::from_secs(10)) {
                Ok(Ok(())) => {
                    let mut state = server_state.lock().unwrap();
                    state.status = ServerStatus::Stopped;
                    state.bind_url = None;
                    state.listener_count = 0;
                    let _ = server_status_tx.try_send(AppServerStatus {
                        running: false,
                        address: None,
                        listener_count: 0,
                    });
                }
                Ok(Err(e)) => {
                    error!("Failed to stop server: {}", e);
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    error!("Server stop timed out after 10 seconds");
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    error!("Server command channel closed");
                }
            }
        });
    }
    
    /// Get server state
    pub fn get_server_state(&self) -> ServerState {
        // Check for updates from channel
        self.poll_server_status();
        let mut state = self.server_state.lock().unwrap();
        // Read real-time listener count from the shared counter (updated by server thread)
        state.listener_count = *self.app_state.listener_count.lock().unwrap();
        state.clone()
    }
    
    /// Poll server status from channel
    fn poll_server_status(&self) {
        match self.server_status_rx.try_recv() {
            Ok(status) => {
                let mut state = self.server_state.lock().unwrap();
                state.listener_count = status.listener_count;
                if status.running && state.status != ServerStatus::Running {
                    state.status = ServerStatus::Running;
                    state.bind_url = status.address;
                } else if !status.running && state.status == ServerStatus::Running {
                    state.status = ServerStatus::Stopped;
                    state.bind_url = None;
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                debug!("Server status channel disconnected");
            }
        }
    }
    
    /// Request server status
    pub fn request_server_status(&self) {
        let (response_tx, response_rx) = bounded(1);
        let cmd = ServerCommand::GetStatus { response: response_tx };
        
        // Non-blocking send to avoid freezing GUI if channel is full
        if let Err(e) = self.app_state.server_cmd_tx.try_send(cmd) {
            error!("Failed to send get status command (channel full): {}", e);
            return;
        }
        
        let server_status_tx = self.server_status_tx.clone();
        std::thread::spawn(move || {
            if let Ok(status) = response_rx.recv_timeout(Duration::from_secs(5)) {
                let _ = server_status_tx.try_send(status);
            }
        });
    }
    
    /// Get server port from config
    pub fn get_server_port(&self) -> u16 {
        self.app_state.config.lock().unwrap().server.port
    }
    
    /// Set server port in config
    pub fn set_server_port(&self, port: u16) {
        self.app_state.config.lock().unwrap().server.port = port;
    }
    
    /// Get server bind address from config
    pub fn get_server_bind_addr(&self) -> String {
        self.app_state.config.lock().unwrap().server.host.clone()
    }
    
    /// Set server bind address in config
    pub fn set_server_bind_addr(&self, addr: String) {
        self.app_state.config.lock().unwrap().server.host = addr;
    }
    
    /// Copied feedback
    pub fn set_copied_feedback(&self, value: bool) {
        *self.copied_feedback.lock().unwrap() = value;
        if value {
            *self.copied_feedback_timer.lock().unwrap() = Some(Instant::now());
        }
    }
    
    pub fn copied_feedback(&self) -> bool {
        let feedback = *self.copied_feedback.lock().unwrap();
        if feedback {
            if let Some(timer) = *self.copied_feedback_timer.lock().unwrap() {
                if timer.elapsed() > Duration::from_secs(2) {
                    *self.copied_feedback.lock().unwrap() = false;
                    *self.copied_feedback_timer.lock().unwrap() = None;
                    return false;
                }
            }
        }
        feedback
    }
    
    /// Open native file dialog to add sound
    pub fn open_file_dialog(&self) {
        if let Some(path) = FileDialog::new()
            .add_filter("Audio Files", &["mp3", "wav", "ogg", "flac", "opus"])
            .set_title("Select Audio File")
            .pick_file()
        {
            self.request_load_sound(path);
        }
    }
    
    /// Update function called each frame - process audio commands on main thread
    pub fn update(&mut self, ctx: &Context) {
        // Process pending audio commands (runs on main thread where AudioEngine lives)
        self.process_audio_commands();
        
        // Auto-detect tracks that finished playing (mixer marks them stopped but keeps them)
        // so the GUI button switches from "Stop" back to "Play" and the sound can be removed.
        if let Ok(mut engine) = self.audio_engine.lock() {
            if let Some(engine) = engine.as_mut() {
                let sounds = self.sounds.lock().unwrap();
                let finished: Vec<SoundId> = sounds
                    .iter()
                    .filter(|(_, entry)| entry.playing)
                    .filter_map(|(id, entry)| {
                        match engine.is_sound_playing(*id) {
                            Ok(false) => Some(*id),
                            Ok(true) | Err(_) => None,
                        }
                    })
                    .collect();
                drop(sounds);
                if !finished.is_empty() {
                    let mut sounds = self.sounds.lock().unwrap();
                    for id in finished {
                        if let Some(entry) = sounds.get_mut(&id) {
                            entry.playing = false;
                            debug!("Auto-stopped finished sound {}", id);
                        }
                    }
                }
            }
        }
        
        // Poll for sound list updates
        match self.sound_list_rx.try_recv() {
            Ok(list) => {
                if !list.is_empty() {
                    self.sync_sound_list(list);
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                debug!("Sound list channel disconnected");
            }
        }
        
        // Poll server status
        self.poll_server_status();
        
        // Request repaint for live updates
        ctx.request_repaint_after(Duration::from_millis(500));
    }
    
    /// Process pending audio commands on the main thread
    fn process_audio_commands(&mut self) {
        // Try to get commands without blocking
        while let Ok(cmd) = self.audio_cmd_rx.try_recv() {
            if let Some(mut engine) = self.audio_engine.lock().unwrap().as_mut() {
                match cmd {
                    AudioCommand::LoadSound { path, response } => {
                        match engine.load_sound(&path) {
                            Ok(sound_id) => {
                                let (duration, _, _) = engine.sound_info(sound_id).unwrap_or((0.0, 48000, 2));
                                let name = std::path::Path::new(&path)
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("Unknown")
                                    .to_string();
                                let info = SoundInfo {
                                    id: sound_id,
                                    name: name.clone(),
                                    path: path.clone(),
                                    duration,
                                    is_playing: false,
                                    volume: 1.0,
                                };
                                // Update local cache
                                let entry = SoundEntry {
                                    id: sound_id,
                                    name,
                                    path: PathBuf::from(&path),
                                    playing: false,
                                    volume: 1.0,
                                };
                                self.sounds.lock().unwrap().insert(sound_id, entry);
                                self.sound_order.lock().unwrap().push(sound_id);
                                let _ = response.send(Ok(info));
                            }
                            Err(e) => {
                                let _ = response.send(Err(e.into()));
                            }
                        }
                    }
                    AudioCommand::PlaySound { id, response } => {
                        let result = engine.play(id);
                        if result.is_ok() {
                            if let Some(entry) = self.sounds.lock().unwrap().get_mut(&id) {
                                entry.playing = true;
                            }
                        }
                        // Convert TrackHandle to () for response
                        let response_result = result.map(|_| ()).map_err(|e| e.into());
                        let _ = response.send(response_result);
                    }
                    AudioCommand::StopSound { id, response } => {
                        let result = engine.stop_sound(id);
                        if result.is_ok() {
                            if let Some(entry) = self.sounds.lock().unwrap().get_mut(&id) {
                                entry.playing = false;
                            }
                        }
                        let _ = response.send(result.map_err(|e| e.into()));
                    }
                    AudioCommand::SetVolume { id, volume, response } => {
                        // For now, set master volume (track volume requires track ID)
                        let result = engine.set_master_volume(volume);
                        if result.is_ok() {
                            if let Some(entry) = self.sounds.lock().unwrap().get_mut(&id) {
                                entry.volume = volume;
                            }
                        }
                        let _ = response.send(result.map_err(|e| e.into()));
                    }
                    AudioCommand::SetMasterVolume { volume, response } => {
                        let result = engine.set_master_volume(volume);
                        if result.is_ok() {
                            *self.master_volume.lock().unwrap() = volume;
                        }
                        let _ = response.send(result.map_err(|e| e.into()));
                    }
                    AudioCommand::RemoveSound { id, response } => {
                        let result = engine.unload_sound(id);
                        if result.is_ok() {
                            self.sounds.lock().unwrap().remove(&id);
                            self.sound_order.lock().unwrap().retain(|&x| x != id);
                        }
                        let _ = response.send(result.map_err(|e| e.into()));
                    }
                    AudioCommand::ListSounds { response } => {
                        let sounds = engine.list_sounds().into_iter().filter_map(|sound_id| {
                            if let Ok((duration, _, _)) = engine.sound_info(sound_id) {
                                Some(SoundInfo {
                                    id: sound_id,
                                    name: format!("Sound {}", sound_id.0),
                                    path: "".to_string(),
                                    duration,
                                    is_playing: false,
                                    volume: 1.0,
                                })
                            } else {
                                None
                            }
                        }).collect();
                        let _ = response.send(sounds);
                    }
                    AudioCommand::StopAll { response } => {
                        let result = engine.stop_all();
                        if result.is_ok() {
                            for entry in self.sounds.lock().unwrap().values_mut() {
                                entry.playing = false;
                            }
                        }
                        let _ = response.send(result.map_err(|e| e.into()));
                    }
                    AudioCommand::IsPlaying { id, response } => {
                        let result = engine.is_sound_playing(id);
                        let _ = response.send(result.unwrap_or(false));
                    }
                }
            } else {
                // Engine not available - handle each variant separately due to different response types
                match cmd {
                    AudioCommand::LoadSound { response, .. } => {
                        let _ = response.send(Err(anyhow::anyhow!("Engine not running")));
                    }
                    AudioCommand::PlaySound { response, .. } => {
                        let _ = response.send(Err(anyhow::anyhow!("Engine not running")));
                    }
                    AudioCommand::StopSound { response, .. } => {
                        let _ = response.send(Err(anyhow::anyhow!("Engine not running")));
                    }
                    AudioCommand::SetVolume { response, .. } => {
                        let _ = response.send(Err(anyhow::anyhow!("Engine not running")));
                    }
                    AudioCommand::SetMasterVolume { response, .. } => {
                        let _ = response.send(Err(anyhow::anyhow!("Engine not running")));
                    }
                    AudioCommand::RemoveSound { response, .. } => {
                        let _ = response.send(Err(anyhow::anyhow!("Engine not running")));
                    }
                    AudioCommand::ListSounds { response } => {
                        let _ = response.send(Vec::new());
                    }
                    AudioCommand::StopAll { response } => {
                        let _ = response.send(Ok(()));
                    }
                    AudioCommand::IsPlaying { response, .. } => {
                        let _ = response.send(false);
                    }
                };
            }
        }
    }
    
    /// Sync local sound list with audio engine
    fn sync_sound_list(&self, sound_infos: Vec<SoundInfo>) {
        let mut sounds = self.sounds.lock().unwrap();
        let mut order = self.sound_order.lock().unwrap();
        
        // Build new order from sound infos
        let new_ids: Vec<SoundId> = sound_infos.iter().map(|s| s.id).collect();
        
        // Update or add sounds
        for info in sound_infos {
            let entry = sounds.entry(info.id).or_insert_with(|| SoundEntry {
                id: info.id,
                name: info.name.clone(),
                path: PathBuf::from(&info.path),
                playing: false,
                volume: 1.0,
            });
            entry.name = info.name;
            entry.path = PathBuf::from(&info.path);
            entry.playing = info.is_playing;
            entry.volume = info.volume;
        }
        
        // Remove sounds that are no longer in the engine
        sounds.retain(|id, _| new_ids.contains(id));
        order.retain(|id| new_ids.contains(id));
        
        // Add new sounds to order
        for id in new_ids {
            if !order.contains(&id) {
                order.push(id);
            }
        }
    }
    
    /// Request full sound list from audio engine
    pub fn request_sound_list(&self) {
        let (response_tx, response_rx) = bounded(1);
        let cmd = AudioCommand::ListSounds { response: response_tx };
        
        if let Err(e) = self.app_state.audio_cmd_tx.try_send(cmd) {
            error!("Failed to send list sounds command (channel full): {}", e);
            return;
        }
        
        let sound_list_tx = self.sound_list_tx.clone();
        std::thread::spawn(move || {
            if let Ok(list) = response_rx.recv_timeout(Duration::from_secs(5)) {
                let _ = sound_list_tx.try_send(list);
            }
        });
    }
}