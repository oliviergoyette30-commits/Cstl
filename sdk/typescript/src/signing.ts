// Signature Ed25519 des messages (specification controle v1, section 4), avec
// le module crypto de Node seulement.
import { createPrivateKey, createPublicKey, generateKeyPairSync, sign, verify, KeyObject } from "node:crypto";
import type { Message } from "./wire.js";

const PKCS8_PREFIX = Buffer.from("302e020100300506032b657004220420", "hex");
const SPKI_PREFIX = Buffer.from("302a300506032b6570032100", "hex");

export function privateKeyFromRaw(raw: Uint8Array): KeyObject {
  if (raw.length !== 32) throw new Error("cle privee Ed25519 : 32 octets attendus");
  return createPrivateKey({ key: Buffer.concat([PKCS8_PREFIX, Buffer.from(raw)]), format: "der", type: "pkcs8" });
}

export function publicKeyFromHex(hex: string): KeyObject {
  const raw = Buffer.from(hex.trim(), "hex");
  if (raw.length !== 32 || raw.toString("hex") !== hex.trim().toLowerCase()) throw new Error("cle publique : 64 caracteres hexadecimaux attendus");
  return createPublicKey({ key: Buffer.concat([SPKI_PREFIX, raw]), format: "der", type: "spki" });
}

export function publicHex(priv: KeyObject): string {
  const der = createPublicKey(priv).export({ format: "der", type: "spki" });
  return Buffer.from(der).subarray(SPKI_PREFIX.length).toString("hex");
}

export function rawPrivate(priv: KeyObject): Buffer {
  return Buffer.from(priv.export({ format: "der", type: "pkcs8" })).subarray(PKCS8_PREFIX.length);
}

export function generatePrivateKey(): KeyObject {
  return generateKeyPairSync("ed25519").privateKey;
}

// Tri par point de code (comme Python sorted() et Rust BTreeMap), pas localeCompare.
function byCodePoint(a: string, b: string): number {
  const x = [...a].map((c) => c.codePointAt(0)!);
  const y = [...b].map((c) => c.codePointAt(0)!);
  for (let i = 0; i < Math.min(x.length, y.length); i++) if (x[i] !== y[i]) return x[i] - y[i];
  return x.length - y.length;
}

const nfc = (s: string) => s.normalize("NFC");

/** Octets canoniques signes (section 4). */
export function signingBytes(m: Message): Buffer {
  const kv = (o: Record<string, string>, skip: string[]) =>
    Object.keys(o).filter((k) => !skip.includes(k)).sort(byCodePoint).map((k) => `|${nfc(k)}=${nfc(o[k])}`).join("");
  const rels = (m.relations ?? [])
    .map((r) => Object.keys(r).sort(byCodePoint).map((k) => `${nfc(k)}=${nfc(r[k])}`).join(","))
    .sort(byCodePoint);
  const text =
    `VERSION|${nfc(m.version)}\nMODE|${nfc(m.mode)}\nMETA${kv(m.meta, ["PARENT_HASH"])}` +
    `\nINTENT${kv(m.intent, ["signature", "rotation_signature"])}\nRELATIONS` + rels.map((r) => `|${r}`).join("");
  return Buffer.from(text, "utf8");
}

export function signMessage(m: Message, priv: KeyObject): string {
  return sign(null, signingBytes(m), priv).toString("hex");
}

export function verifyHex(data: Uint8Array, signatureHex: string, publicKeyHex: string): boolean {
  try {
    return verify(null, data, publicKeyFromHex(publicKeyHex), Buffer.from(signatureHex, "hex"));
  } catch {
    return false;
  }
}
