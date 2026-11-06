#!/usr/bin/env node
/**
 * Verifies that a built Windows executable really carries the application icon.
 *
 * Walks the PE resource directory by hand (RT_GROUP_ICON -> GRPICONDIR ->
 * RT_ICON) and prints every resolution present, so "the icon is embedded" is
 * checked against the binary instead of assumed from a successful build.
 *
 * Usage: node assets/tools/verify-exe-icon.mjs [path-to-exe]
 *        (defaults to target/debug/soundshared.exe)
 */

import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const exePath = process.argv[2] ?? path.join(ROOT, 'target', 'debug', 'soundshared.exe');

const RT_ICON = 3;
const RT_GROUP_ICON = 14;

const buf = readFileSync(exePath);

// --- PE headers ------------------------------------------------------------
if (buf.readUInt16LE(0) !== 0x5a4d) {
  throw new Error('not a PE file: missing MZ signature');
}

const peOffset = buf.readUInt32LE(0x3c);
if (buf.toString('ascii', peOffset, peOffset + 4) !== 'PE\0\0') {
  throw new Error('not a PE file: missing PE signature');
}

const coff = peOffset + 4;
const sectionCount = buf.readUInt16LE(coff + 2);
const optionalSize = buf.readUInt16LE(coff + 16);
const optional = coff + 20;
const magic = buf.readUInt16LE(optional);
const dataDirectory = optional + (magic === 0x20b ? 112 : 96);

const resourceRva = buf.readUInt32LE(dataDirectory + 2 * 8);
if (resourceRva === 0) {
  console.error(`FAIL  ${exePath}: no resource directory (no icon embedded)`);
  process.exit(1);
}

const sections = [];
for (let i = 0; i < sectionCount; i++) {
  const s = optional + optionalSize + i * 40;
  sections.push({
    virtualAddress: buf.readUInt32LE(s + 12),
    sizeOfRawData: buf.readUInt32LE(s + 16),
    pointerToRawData: buf.readUInt32LE(s + 20),
  });
}

/** Maps an RVA inside the resource section to a file offset. */
function rvaToOffset(rva) {
  for (const section of sections) {
    const end = section.virtualAddress + Math.max(section.sizeOfRawData, 1);
    if (rva >= section.virtualAddress && rva < end) {
      return section.pointerToRawData + (rva - section.virtualAddress);
    }
  }
  throw new Error(`RVA 0x${rva.toString(16)} is not in any section`);
}

// --- Resource directory ----------------------------------------------------
const resourceBase = rvaToOffset(resourceRva);

/** Lists {id, isDirectory, offset} entries of one IMAGE_RESOURCE_DIRECTORY. */
function readDirectory(base) {
  const named = buf.readUInt16LE(base + 12);
  const ids = buf.readUInt16LE(base + 14);
  const entries = [];
  for (let i = 0; i < named + ids; i++) {
    const at = base + 16 + i * 8;
    const nameOrId = buf.readUInt32LE(at);
    const offset = buf.readUInt32LE(at + 4);
    entries.push({
      id: nameOrId & 0x80000000 ? null : nameOrId,
      isDirectory: Boolean(offset & 0x80000000),
      offset: resourceBase + (offset & 0x7fffffff),
    });
  }
  return entries;
}

const typeEntry = readDirectory(resourceBase).find((e) => e.id === RT_GROUP_ICON);
if (!typeEntry) {
  console.error(`FAIL  ${exePath}: no RT_GROUP_ICON resource (no icon embedded)`);
  process.exit(1);
}

const groups = [];
for (const nameEntry of readDirectory(typeEntry.offset)) {
  // Third level is the language; any one of them points at the data.
  const language = readDirectory(nameEntry.offset)[0];
  const dataRva = buf.readUInt32LE(language.offset);
  const dataSize = buf.readUInt32LE(language.offset + 4);
  const dataOffset = rvaToOffset(dataRva);

  const count = buf.readUInt16LE(dataOffset + 4);
  const icons = [];
  for (let i = 0; i < count; i++) {
    const at = dataOffset + 6 + i * 14;
    const width = buf[at] === 0 ? 256 : buf[at];
    const height = buf[at + 1] === 0 ? 256 : buf[at + 1];
    const bitCount = buf.readUInt16LE(at + 6);
    const bytes = buf.readUInt32LE(at + 8);
    icons.push({ id: buf.readUInt16LE(at + 12), width, height, bitCount, bytes });
  }
  groups.push({ id: nameEntry.id, dataSize, icons });
}

// --- Cross-check each GRPICONDIR entry against RT_ICON ---------------------
const iconEntry = readDirectory(resourceBase).find((e) => e.id === RT_ICON);
const available = new Set(
  iconEntry ? readDirectory(iconEntry.offset).map((e) => e.id) : [],
);

let failures = 0;
console.log(`${exePath}`);
for (const group of groups) {
  console.log(`  RT_GROUP_ICON ${group.id}  (${group.dataSize} B, ${group.icons.length} resolutions)`);
  for (const icon of group.icons) {
    const present = available.has(icon.id);
    if (!present) failures++;
    console.log(
      `    ${present ? 'ok  ' : 'MISS'} id=${String(icon.id).padEnd(4)} ` +
        `${icon.width}x${icon.height} ${icon.bitCount}bpp ${icon.bytes} B`,
    );
  }
}

if (failures > 0) {
  console.error(`\nFAIL  ${failures} icon entries reference a missing RT_ICON resource`);
  process.exit(1);
}

const sizes = groups.flatMap((g) => g.icons.map((i) => i.width)).sort((a, b) => a - b);
console.log(`\nOK    icon embedded: ${sizes.join(', ')} px`);
