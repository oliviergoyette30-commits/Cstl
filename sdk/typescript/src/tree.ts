// Arbres (donnees imbriquees) <-> relations CSTL, identiques au SDK Python
// (cstl_compact.tree_to_relations / relations_to_tree / tree_hash), 2026-11-25.
// Chaque element d'une liste d'objets devient une relation marquee tree=<id> ;
// le serveur verifie les comptes (tree_n) et, avec une source, l'empreinte
// engagee (tree_commit). Les nombres entiers sont de type int, les autres num.
import { createHash } from "node:crypto";

export type Json = null | boolean | number | string | Json[] | { [k: string]: Json };
export type Relation = Record<string, string>;

export class TreeError extends Error {}

const NAME_OK = /^[A-Za-z0-9_.\-]{1,128}$/;
const RESERVED = new Set(["type", "subject", "object", "modality", "id", "sigma", "tau", "layer", "weight", "coref_with"]);
export const WIRE_CHUNK = 250;

const isObj = (v: unknown): v is { [k: string]: Json } => typeof v === "object" && v !== null && !Array.isArray(v);
const isObjList = (v: unknown): v is { [k: string]: Json }[] => Array.isArray(v) && v.length > 0 && v.every(isObj);

/** JSON compact, comme json.dumps(..., ensure_ascii=False, separators=(",", ":")) en Python. */
function pyJson(v: Json): string {
  if (typeof v === "number") return pyNum(v);
  if (Array.isArray(v)) return "[" + v.map(pyJson).join(",") + "]";
  if (isObj(v)) return "{" + Object.entries(v).map(([k, x]) => JSON.stringify(k) + ":" + pyJson(x)).join(",") + "}";
  return JSON.stringify(v);
}

function pyNum(n: number): string {
  if (!Number.isFinite(n)) throw new TreeError(`nombre non representable en JSON : ${n}`);
  return String(n);
}

function b64(text: string): string {
  return Buffer.from(text, "utf8").toString("base64url");
}

function unb64(text: string): string {
  return Buffer.from(text, "base64url").toString("utf8");
}

function fieldName(k: string): string {
  if (!NAME_OK.test(k)) throw new TreeError(`nom de champ non transmissible au serveur : ${JSON.stringify(k)}`);
  return RESERVED.has(k) || k.startsWith("f_") || k.startsWith("tree") ? `f_${k}` : k;
}

const unfield = (k: string) => (k.startsWith("f_") ? k.slice(2) : k);

function wireValue(v: Json): [string, string | null] {
  if (v === null) return ["", "null"];
  if (v === true || v === false) return [v ? "true" : "false", "bool"];
  if (typeof v === "number") return Number.isInteger(v) ? [pyNum(v), "int"] : [pyNum(v), "num"];
  if (typeof v === "object" || /["[\]\n\r\\]/.test(v)) return [b64(pyJson(v)), "json"];
  return [v, null];
}

function putWire(rel: Relation, types: string[], name: string, v: Json): void {
  const [out, t0] = wireValue(v);
  const cps = Array.from(out); // morceaux en points de code, comme len() en Python
  const parts: string[] = [];
  for (let i = 0; i < cps.length; i += WIRE_CHUNK) parts.push(cps.slice(i, i + WIRE_CHUNK).join(""));
  if (parts.length === 0) parts.push("");
  rel[name] = parts[0];
  parts.slice(1).forEach((p, i) => { rel[`tree_c${i + 1}_${name}`] = p; });
  const t = parts.length > 1 ? `${t0 ?? "str"}/${parts.length}` : t0;
  if (t) types.push(`${name}:${t}`);
}

function putTypes(rel: Relation, types: string[]): void {
  const chunks: string[] = [];
  let cur = "";
  for (const t of types) {
    if (cur && cur.length + 1 + t.length > WIRE_CHUNK) { chunks.push(cur); cur = t; } else cur = cur ? `${cur} ${t}` : t;
  }
  if (cur) chunks.push(cur);
  chunks.forEach((c, i) => { rel[i === 0 ? "tree_types" : `tree_t${i}`] = c; });
}

function fromWire(v: string, t: string | undefined): Json {
  if (t === "json") return JSON.parse(unb64(v)) as Json;
  if (t === "null") return null;
  if (t === "bool") return v === "true";
  if (t === "int" || t === "num") return Number(v);
  return v;
}

function getWire(rel: Relation, name: string, t: string | undefined): Json {
  let v = rel[name];
  if (t && t.includes("/")) {
    const [base, n] = t.split("/", 2);
    for (let i = 1; i < Number(n); i++) {
      const part = rel[`tree_c${i}_${name}`];
      if (part === undefined) throw new TreeError(`valeur longue incomplete : ${name} (${n} morceaux annonces)`);
      v += part;
    }
    t = base === "str" ? undefined : base;
  }
  return fromWire(v, t);
}

/** Document JSON -> relations marquees tree=<treeId> (cstl_compact.tree_to_relations). */
export function treeToRelations(data: Json, treeId: string, opts: { source?: string; nonce?: string } = {}): Relation[] {
  if (!NAME_OK.test(treeId)) throw new TreeError(`id d'arbre invalide : ${treeId}`);
  const rels: Relation[] = [];
  const seen = new Set<string>();
  let kind: string;
  let extra: { [k: string]: Json } = {};
  let lists: [string, { [k: string]: Json }[]][] = [];
  if (isObjList(data)) {
    kind = "list";
    lists = [["_", data]];
  } else if (isObj(data)) {
    for (const [k, v] of Object.entries(data)) {
      if (NAME_OK.test(k) && isObjList(v)) lists.push([k, v]);
      else extra[k] = v;
    }
    kind = Object.keys(extra).length ? "fields" : "tables";
  } else {
    kind = "value";
    extra = { v: data };
  }
  const root: Relation = { type: "tree_root", subject: "tree_root", object: "-", tree: treeId, tree_kind: kind };
  if (opts.source !== undefined) {
    if (!NAME_OK.test(opts.source)) throw new TreeError(`nom de source invalide : ${opts.source}`);
    root.tree_source = opts.source;
  }
  if (opts.nonce !== undefined) {
    if (!/^[0-9a-f]{32,128}$/.test(opts.nonce)) throw new TreeError("nonce : 32 a 128 chiffres hexadecimaux");
    root.tree_nonce = opts.nonce;
  }
  const rootTypes: string[] = [];
  for (const [k, v] of Object.entries(extra)) putWire(root, rootTypes, fieldName(k), v);
  if (rootTypes.length) putTypes(root, rootTypes);
  root.tree_n = String(lists.reduce((n, [, v]) => n + v.length, 0));
  rels.push(root);

  const walk = (key: string, objs: { [k: string]: Json }[], parent: string) => {
    if (!NAME_OK.test(key)) throw new TreeError(`nom de liste non transmissible : ${key}`);
    objs.forEach((o, i) => {
      const own = typeof o.id === "string" && o.id !== "" && !o.id.includes(".");
      const oid = own ? (o.id as string) : parent !== "-" ? `${parent}.${i}` : `${key}.${i}`;
      if (seen.has(oid)) throw new TreeError(`id en double dans l'arbre : ${oid}`);
      seen.add(oid);
      const rel: Relation = { type: key, subject: oid, object: parent, tree: treeId };
      if (own) rel.tree_id = "1";
      const types: string[] = [];
      const kids: [string, { [k: string]: Json }[]][] = [];
      for (const [k, v] of Object.entries(o)) {
        if (own && k === "id") continue;
        if (isObjList(v)) { kids.push([k, v]); continue; }
        putWire(rel, types, fieldName(k), v);
      }
      if (types.length) putTypes(rel, types);
      if (kids.length) rel.tree_n = String(kids.reduce((n, [, v]) => n + v.length, 0));
      rels.push(rel);
      for (const [k, v] of kids) walk(k, v, oid);
    });
  };
  for (const [key, objs] of lists) walk(key, objs, "-");
  return rels;
}

const SKIP = new Set(["type", "subject", "object", "tree", "tree_id", "tree_types", "tree_kind", "tree_n", "tree_source", "tree_nonce", "block"]);

/** Inverse de treeToRelations (relations dans l'ordre d'envoi) ; verifie les comptes tree_n. */
export function relationsToTree(rels: Relation[], treeId?: string): Json {
  const mine = rels.filter((r) => treeId === undefined || r.tree === treeId);
  const nodes = new Map<string, { [k: string]: Json }>();
  const root: { [k: string]: Json[] } = {};
  let kind: string | undefined;
  let extra: { [k: string]: Json } = {};
  for (const r of mine) {
    const all = [r.tree_types ?? ""];
    for (let i = 1; r[`tree_t${i}`] !== undefined; i++) all.push(r[`tree_t${i}`]);
    const types = new Map<string, string>();
    for (const t of all.join(" ").split(/\s+/)) {
      const j = t.indexOf(":");
      if (j > 0) types.set(t.slice(0, j), t.slice(j + 1));
    }
    const o: { [k: string]: Json } = r.tree_id === "1" ? { id: r.subject } : {};
    for (const k of Object.keys(r)) {
      if (SKIP.has(k) || k.startsWith("tree_c") || k.startsWith("tree_t")) continue;
      o[unfield(k)] = getWire(r, k, types.get(k));
    }
    if (r.tree_kind) { kind = r.tree_kind; extra = o; continue; }
    nodes.set(r.subject, o);
    if (r.object === "-") (root[r.type] ??= []).push(o);
    else if (nodes.has(r.object)) {
      const p = nodes.get(r.object)!;
      ((p[r.type] ??= []) as Json[]).push(o);
    } else throw new TreeError(`parent inconnu ${r.object} pour ${r.subject}`);
  }
  const kids = new Map<string, number>();
  for (const r of mine) if (!r.tree_kind) kids.set(r.object, (kids.get(r.object) ?? 0) + 1);
  for (const r of mine) {
    if (r.tree_n === undefined) continue;
    const got = kids.get(r.tree_kind ? "-" : r.subject) ?? 0;
    if (String(got) !== r.tree_n) throw new TreeError(`arbre incomplet : ${r.subject} annonce ${r.tree_n} enfant(s), ${got} relu(s)`);
  }
  if (kind === "value") return extra.v ?? null;
  if (kind === "list") return root._ ?? [];
  return { ...extra, ...root };
}

/** Empreinte canonique (cstl_compact.tree_hash, tree_check::canonical_hash cote serveur). */
export function treeHash(rels: Relation[], treeId: string): string {
  const cmp = (a: string, b: string) => {
    const x = Array.from(a), y = Array.from(b);
    for (let i = 0; i < Math.min(x.length, y.length); i++) {
      const d = x[i].codePointAt(0)! - y[i].codePointAt(0)!;
      if (d) return d;
    }
    return x.length - y.length;
  };
  const body = rels.filter((r) => r.tree === treeId)
    .map((r) => Object.keys(r).filter((k) => k !== "block").sort(cmp).map((k) => `${k}=${r[k]}`).join("\x1f"))
    .join("\x1e");
  return "sha256:" + createHash("sha256").update(body, "utf8").digest("hex");
}
