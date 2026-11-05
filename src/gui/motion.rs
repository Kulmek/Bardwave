//! Reduced-motion policy for the console.
//!
//! The design system requires the app to respect reduced motion — the live-dot
//! pulse and the backdrop scanline stop, while state changes stay instant. egui
//! 0.30 exposes no such signal (there is no `reduce_motion` option, and
//! `SPI_GETCLIENTAREAANIMATION` would mean a new platform dependency), so the
//! operator states it, in one of two places:
//!
//! 1. `config.toml` → `[gui] reduce_motion = true` (persistent, default `false`);
//! 2. `SOUNDSHARED_REDUCED_MOTION=1` in the environment (per session, wins over
//!    the config file, so a single run can be checked without editing anything).
//!
//! Both are read once at startup: motion is a presentation choice, not state
//! that changes while the console is open.

/// Environment variable that overrides `[gui] reduce_motion` for one session.
pub const REDUCED_MOTION_ENV: &str = "SOUNDSHARED_REDUCED_MOTION";

/// Decide whether decorative motion is off.
///
/// Pure so it can be tested without touching the process environment: `env` is
/// the raw value of [`REDUCED_MOTION_ENV`], or `None` when it is unset.
/// Unrecognised values are ignored and fall back to `configured`, so a typo can
/// never silently change how the console animates.
pub fn resolve_reduced_motion(configured: bool, env: Option<&str>) -> bool {
    match env.map(str::trim) {
        Some(value)
            if value.eq_ignore_ascii_case("1")
                || value.eq_ignore_ascii_case("true")
                || value.eq_ignore_ascii_case("yes")
                || value.eq_ignore_ascii_case("on") =>
        {
            true
        }
        Some(value)
            if value.eq_ignore_ascii_case("0")
                || value.eq_ignore_ascii_case("false")
                || value.eq_ignore_ascii_case("no")
                || value.eq_ignore_ascii_case("off") =>
        {
            false
        }
        _ => configured,
    }
}

/// [`resolve_reduced_motion`] with the process environment read for you.
pub fn reduced_motion(configured: bool) -> bool {
    let env = std::env::var(REDUCED_MOTION_ENV).ok();
    resolve_reduced_motion(configured, env.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_alone_decides_when_the_env_var_is_unset() {
        assert!(!resolve_reduced_motion(false, None));
        assert!(resolve_reduced_motion(true, None));
    }

    #[test]
    fn the_env_var_overrides_the_config_in_both_directions() {
        assert!(resolve_reduced_motion(false, Some("1")));
        assert!(resolve_reduced_motion(false, Some(" TRUE ")));
        assert!(!resolve_reduced_motion(true, Some("0")));
        assert!(!resolve_reduced_motion(true, Some("off")));
    }

    #[test]
    fn unrecognised_env_values_fall_back_to_the_config() {
        for typo in ["", "  ", "maybe", "2", "reduced"] {
            assert_eq!(
                resolve_reduced_motion(true, Some(typo)),
                true,
                "{typo:?} must not switch motion off"
            );
            assert_eq!(
                resolve_reduced_motion(false, Some(typo)),
                false,
                "{typo:?} must not switch motion on"
            );
        }
    }
}
