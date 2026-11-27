// Format texte CSTL (specification controle v1, sections 2 et 3).

export type Fields = Record<string, string>;

export interface Message {
  version: string;
  mode: string;
  meta: Fields;
  intent: Fields;
  relations?: Fields[];
}

export interface Block {
  name: string;
  fields: Fields;
}

/** Erreur d'une valeur ou d'une cle que le fil CSTL ne peut pas porter a l'identique. */
export class WireError extends Error {}

const BAD_KEY = /[\s=,[\]"]/;

function wireKey(k: string): string {
  if (!k || BAD_KEY.test(k)) throw new WireError(`cle invalide : ${JSON.stringify(k)}`);
  return k;
}

/** Section 3.2 : une valeur interdite est refusee, jamais modifiee. */
export function wireValue(v: string): string {
  if (typeof v !== "string") throw new WireError(`la valeur doit etre une chaine : ${JSON.stringify(v)}`);
  if (v.includes('"')) throw new WireError(`guillemet non transportable : ${JSON.stringify(v)}`);
  if (/[\r\n]/.test(v)) throw new WireError(`retour a la ligne non transportable : ${JSON.stringify(v)}`);
  if (v.trim() !== v) throw new WireError(`espaces en bord de valeur : ${JSON.stringify(v)}`);
  if (v.includes("---END---")) throw new WireError("la valeur contient ---END---");
  return /[,[\]]/.test(v) ? `"${v}"` : v;
}

function block(name: string, f: Fields): string {
  return `${name} [` + Object.entries(f).map(([k, v]) => `${wireKey(k)}=${wireValue(v)}`).join(", ") + "]\n";
}

export function render(m: Message): string {
  let out = `#!CSTL ${m.version} MODE=${m.mode}\n` + block("META", m.meta) + block("INTENT_PAYLOAD", m.intent);
  for (const r of m.relations ?? []) out += block("RELATION", r);
  return out + "---END---\n";
}

/** Section 3.3 : virgules hors guillemets, premier "=", espaces et guillemets retires. */
export function splitFields(content: string): Fields {
  const parts: string[] = [];
  let cur = "";
  let quoted = false;
  for (const ch of content) {
    if (ch === '"') quoted = !quoted;
    if (ch === "," && !quoted) {
      parts.push(cur);
      cur = "";
    } else cur += ch;
  }
  parts.push(cur);
  const out: Fields = {};
  for (const p of parts) {
    const i = p.indexOf("=");
    if (i < 0) continue;
    out[p.slice(0, i).trim()] = p.slice(i + 1).trim().replace(/^"+|"+$/g, "");
  }
  return out;
}

/** Reponse -> blocs, dans l'ordre. */
export function parse(text: string): Block[] {
  const blocks: Block[] = [];
  for (const raw of text.split("\n").slice(1)) {
    const line = raw.trim();
    if (line === "---END---") break;
    const i = line.indexOf(" [");
    if (i > 0 && line.endsWith("]")) blocks.push({ name: line.slice(0, i), fields: splitFields(line.slice(i + 2, -1)) });
  }
  return blocks;
}

export function first(blocks: Block[], name: string): Fields {
  return blocks.find((b) => b.name === name)?.fields ?? {};
}
