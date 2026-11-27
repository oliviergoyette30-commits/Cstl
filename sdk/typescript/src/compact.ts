// CSTL compact (relations et politiques), identique au SDK Python
// (sdk/python/cstl_compact.py : encode, encode_symbols, decode, decode_report,
// policy_to_compact, policy_to_symbols), 2026-11-25. Rien n'est devine : une
// ligne illisible, un nombre [N] faux ou une valeur mal typee leve CompactError.

export class CompactError extends Error {}

export type Rel = Record<string, string>;
type Word = [string, boolean]; // [texte, entierement entre guillemets]

export const MODALITIES = ["MUST", "MUST_NOT", "MAY", "REQUIRE", "FORBID", "PERMIT"];
const PREFIXES: [string, string][] = [["total<=", "total_at_most"], ["total>", "total_above"], ["<=", "amount_at_most"],
  [">", "amount_above"], ["window=", "total_window_secs"], ["if:", "if_fact"], ["unless:", "unless_fact"],
  ["from:", "valid_from"], ["until:", "valid_until"]];
const SHORT: Record<string, string> = Object.fromEntries(PREFIXES.map(([p, a]) => [a, p]));
export const TYPES = ["num", "int", "date", "bool", "str"];
const DATE = /^\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:Z|[+-]\d{2}:?\d{2})?)?$/;
const CONDITION_TYPES: Record<string, string> = { amount_above: "num", amount_at_most: "num", total_above: "num",
  total_at_most: "num", total_window_secs: "int", valid_from: "date", valid_until: "date", priority: "int" };
const SYMBOLS: Record<string, string> = { MUST: "*", MUST_NOT: "!", MAY: "?" };
const SYMBOL_MODALITY: Record<string, string> = { "*": "MUST", "!": "MUST_NOT", "?": "MAY" };
const HEADER_SYM = /^([A-Za-z_][A-Za-z0-9_.]*)\[(\d+)\]:((?:\s+\S+)*)\s*$/;
const PLAIN = /^[A-Za-z0-9_.:\-]+$/;
const SECTION = /^(ROLES|FACTS|RULES)(?:\[(\d+)\])?(?:\s.*)?$/;
const GENERIC_TYPE = /^[A-Za-z_][A-Za-z0-9_.]*$/;
const GENERIC = /^([A-Za-z_][A-Za-z0-9_.]*)\[(\d+)\]\s+subject\s+object((?:\s+\S+)*)\s*$/;
const isSpace = (c: string) => /\s/u.test(c);
const LINE_BREAK = new RegExp("\\r\\n|[\\n\\r\\x0b\\x0c\\x1c\\x1d\\x1e\\x85\\u2028\\u2029]");
const splitLines = (t: string) => t.split(LINE_BREAK).filter((_, i, a) => i < a.length - 1 || a[i] !== "");

/** La valeur est-elle du type `ty` ? (memes regles que le serveur, src/server/schema.rs). */
export function checkValue(ty: string, raw: string): boolean {
  const v = String(raw).trim();
  if (ty === "num") {
    const D = String.raw`\d(?:_?\d)*`;
    if (!new RegExp(`^[+-]?(?:${D}(?:\\.(?:${D})?)?|\\.${D})(?:[eE][+-]?${D})?$`).test(v)) return false;
    return Number.isFinite(Number(v.replace(/_/g, "")));
  }
  if (ty === "int") return /^[+-]?\d+$/.test(v);
  if (ty === "date") {
    if (!DATE.test(v)) return false;
    const [y, m, d] = v.slice(0, 10).split("-").map(Number);
    if (y < 1 || m < 1 || m > 12 || d < 1) return false;
    const days = [31, (y % 4 === 0 && y % 100 !== 0) || y % 400 === 0 ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    return d <= days[m - 1];
  }
  if (ty === "bool") return ["true", "false", "1", "0"].includes(v);
  return ty === "str";
}

function checkRule(r: Rel, n: number): Rel {
  for (const [attr, ty] of Object.entries(CONDITION_TYPES)) {
    if (attr in r && !checkValue(ty, r[attr])) throw new CompactError(`ligne ${n} : ${attr}=${JSON.stringify(r[attr])} n'est pas de type ${ty}`);
  }
  return r;
}

type Types = Record<string, Record<string, string>>;

function checkTyped(rels: Rel[], types: Types): void {
  rels.forEach((r, i) => {
    if (r.type === "PERFORM") checkRule(r, i + 1);
    for (const [col, ty] of Object.entries(types[r.type] ?? {})) {
      if (col in r && r[col] !== "" && !checkValue(ty, r[col])) {
        throw new CompactError(`relation ${i + 1} : ${r.type}.${col}=${JSON.stringify(r[col])} n'est pas de type ${ty}`);
      }
    }
  });
}

function schemaTypes(rels: Rel[]): Types {
  const out: Types = {};
  for (const r of rels) {
    if (r.type === "SCHEMA" && r.subject) {
      out[r.subject] = Object.fromEntries(Object.entries(r).filter(([k, v]) => !["type", "subject", "object"].includes(k) && TYPES.includes(v)));
    }
  }
  return out;
}

const col = (c: string, types?: Record<string, string>) => (types?.[c] && types[c] !== "str" ? `${c}:${types[c]}` : c);

function parseCols(names: string[], n: number): [string[], Record<string, string>] {
  const cols: string[] = [], types: Record<string, string> = {};
  for (const raw of names) {
    const i = raw.indexOf(":");
    const name = i >= 0 ? raw.slice(0, i) : raw;
    if (i >= 0) {
      const ty = raw.slice(i + 1);
      if (!TYPES.includes(ty)) throw new CompactError(`ligne ${n} : type inconnu ${JSON.stringify(ty)} pour la colonne ${name} (types : ${TYPES.join(", ")})`);
      types[name] = ty;
    }
    cols.push(name);
  }
  return [cols, types];
}

const quote = (t: string) => '"' + String(t).replace(/\\/g, "\\\\").replace(/"/g, '\\"') + '"';
const token = (v: string) => (v && !/[\s"#]/u.test(v) ? v : quote(v));

function key(k: string): string {
  if (k.includes("=")) throw new CompactError(`nom d'attribut avec « = » non representable : ${JSON.stringify(k)}`);
  return token(k);
}

function ruleExtras(r: Rel, skip: string[]): string[] {
  const parts: string[] = [];
  for (const [k, v] of Object.entries(r)) {
    if (skip.includes(k) || k === "reason") continue;
    parts.push(k in SHORT ? SHORT[k] + token(v) : `${key(k)}=${token(v)}`);
  }
  if ("reason" in r) parts.push(quote(r.reason));
  return parts;
}

const keysAre = (r: Rel, ks: string[]) => Object.keys(r).length === ks.length && ks.every((k) => k in r);
const isRole = (r: Rel) => r.type === "POSSESSES" && (r.object ?? "").startsWith("role:") && keysAre(r, ["type", "subject", "object"]);
const isFact = (r: Rel) => r.type === "STATE" && ["true", "false"].includes(r.object) && keysAre(r, ["type", "subject", "object"]);
const isRule = (r: Rel) => r.type === "PERFORM" && MODALITIES.includes(r.modality) && !!r.subject && !!r.object;

function line(r: Rel): string {
  if (isRole(r)) return `ROLE ${token(r.subject)} ${token(r.object.slice(5))}`;
  if (isFact(r)) return `FACT ${token(r.subject)} ${r.object}`;
  if (isRule(r)) return [token(r.subject), r.modality, token(r.object), ...ruleExtras(r, ["type", "subject", "object", "modality"])].join(" ");
  const rest = Object.entries(r).filter(([k]) => !["type", "subject", "object"].includes(k)).map(([k, v]) => `${key(k)}=${token(v)}`);
  return ["REL", token(r.type ?? ""), token(r.subject ?? ""), token(r.object ?? ""), ...rest].join(" ");
}

const cell = (v: string | undefined) => (v === undefined || v === "" ? "-" : v === "-" ? quote(v) : token(v));
const norm = (relations: Record<string, unknown>[]): Rel[] => relations.map((r) => Object.fromEntries(Object.entries(r).map(([k, v]) => [k, String(v)])));

const genericOk = (r: Rel) => r.subject !== undefined && r.object !== undefined && GENERIC_TYPE.test(r.type ?? "")
  && !["ROLES", "FACTS", "RULES"].includes(r.type) && Object.keys(r).every((k) => ["type", "subject", "object"].includes(k) || GENERIC_TYPE.test(k));

function genericSections(groups: Map<string, Rel[]>, types: Types, header: (t: string, n: number, cols: string) => string): string[] {
  const lines: string[] = [];
  for (const [t, rs] of groups) {
    const cols: string[] = [];
    for (const r of rs) for (const k of Object.keys(r)) if (!["type", "subject", "object"].includes(k) && !cols.includes(k)) cols.push(k);
    lines.push(header(t, rs.length, cols.map((c) => " " + col(c, types[t])).join("")));
    for (const r of rs) {
      lines.push("  " + [r.subject !== "" ? cell(r.subject) : '""', r.object !== "" ? cell(r.object) : '""',
        ...cols.map((c) => (r[c] !== "" ? cell(r[c]) : '""'))].join(" "));
    }
  }
  return lines;
}

/** Relations -> CSTL compact (mode ligne, ou tableau avec `table`). */
export function encode(relations: Record<string, unknown>[], opts: { table?: boolean; types?: Types } = {}): string {
  const rels = norm(relations);
  const types = { ...schemaTypes(rels), ...(opts.types ?? {}) };
  checkTyped(rels, types);
  if (!opts.table) return rels.map(line).join("\n");
  const roles = rels.filter(isRole), facts = rels.filter(isFact), rules = rels.filter(isRule);
  const others = rels.filter((r) => !(isRole(r) || isFact(r) || isRule(r)));
  const lines: string[] = [];
  if (roles.length) {
    lines.push(`ROLES[${roles.length}] agent role`);
    for (const r of roles) lines.push(`  ${cell(r.subject)} ${cell(r.object.slice(5))}`);
  }
  if (facts.length) {
    lines.push(`FACTS[${facts.length}] fait valeur`);
    for (const r of facts) lines.push(`  ${cell(r.subject)} ${r.object}`);
  }
  if (rules.length) {
    lines.push(`RULES[${rules.length}] qui modalite action au_dela si sauf_si`);
    for (const r of rules) {
      const cells = [cell(r.subject), r.modality, cell(r.object), cell(r.amount_above), cell(r.if_fact), cell(r.unless_fact)];
      lines.push("  " + [...cells, ...ruleExtras(r, ["type", "subject", "object", "modality", "amount_above", "if_fact", "unless_fact"])].join(" "));
    }
  }
  const groups = new Map<string, Rel[]>();
  for (const r of others) {
    if (!genericOk(r)) { lines.push(line(r)); continue; }
    if (!groups.has(r.type)) groups.set(r.type, []);
    groups.get(r.type)!.push(r);
  }
  return [...lines, ...genericSections(groups, types, (t, n, cols) => `${t}[${n}] subject object${cols}`)].join("\n");
}

function symLine(r: Rel): string | null {
  if (isRole(r) && PLAIN.test(r.subject) && PLAIN.test(r.object.slice(5))) return `${r.subject} @${r.object.slice(5)}`;
  if (isFact(r) && PLAIN.test(r.subject)) return `${r.subject}=${r.object === "true" ? "1" : "0"}`;
  if (isRule(r) && r.modality in SYMBOLS && PLAIN.test(r.subject) && PLAIN.test(r.object)
      && !"*!?@".includes(r.object[0]) && !r.subject.includes("=")) {
    const parts = [r.subject, SYMBOLS[r.modality] + r.object];
    if ("amount_above" in r) parts.push(">" + token(r.amount_above));
    for (const [attr, sym] of [["if_fact", "+"], ["unless_fact", "-"]]) {
      if (attr in r) {
        if (!PLAIN.test(r[attr])) return null;
        parts.push(sym + r[attr]);
      }
    }
    return [...parts, ...ruleExtras(r, ["type", "subject", "object", "modality", "amount_above", "if_fact", "unless_fact"])].join(" ");
  }
  return null;
}

/** CSTL compact SANS MOT-CLE (* MUST, ! MUST_NOT, ? MAY, @role, fait=1|0, tableaux par type). */
export function encodeSymbols(relations: Record<string, unknown>[], types: Types = {}): string {
  const rels = norm(relations);
  const all = { ...schemaTypes(rels), ...types };
  checkTyped(rels, all);
  const lines: string[] = [];
  const groups = new Map<string, Rel[]>();
  for (const r of rels) {
    const policy = isRole(r) || isFact(r) || isRule(r);
    const sym = policy ? symLine(r) : null;
    if (sym !== null) lines.push(sym);
    else if (!policy && genericOk(r)) {
      if (!groups.has(r.type)) groups.set(r.type, []);
      groups.get(r.type)!.push(r);
    } else lines.push(line(r));
  }
  return [...lines, ...genericSections(groups, all, (t, n, cols) => `${t}[${n}]:${cols}`)].join("\n");
}

function split(l: string): Word[] {
  const toks: Word[] = [];
  const ch = Array.from(l);
  let cur: string[] = [], quotedWhole = false, inTok = false, i = 0;
  while (i < ch.length) {
    const c = ch[i];
    if (c === '"') {
      if (!inTok) { inTok = true; quotedWhole = true; }
      i++;
      while (i < ch.length && ch[i] !== '"') {
        if (ch[i] === "\\" && i + 1 < ch.length) i++;
        cur.push(ch[i]);
        i++;
      }
      if (i >= ch.length) throw new CompactError(`guillemets mal fermes : ${JSON.stringify(l)}`);
      i++;
      continue;
    }
    if (isSpace(c) || c === "#") {
      if (inTok) { toks.push([cur.join(""), quotedWhole]); cur = []; quotedWhole = false; inTok = false; }
      if (c === "#") break;
      i++;
      continue;
    }
    if (!inTok) { inTok = true; quotedWhole = false; } else if (quotedWhole) quotedWhole = false;
    cur.push(c);
    i++;
  }
  if (inTok) toks.push([cur.join(""), quotedWhole]);
  return toks;
}

const empty = (w: Word) => (w[0] === "-" && !w[1] ? null : w[0]);

function ruleFrom(toks: string[], words: Word[], n: number, base: Rel): Rel {
  const r = { ...base };
  let rest = toks;
  if (rest.length && words[words.length - 1][1]) { r.reason = rest[rest.length - 1]; rest = rest.slice(0, -1); }
  for (const t of rest) {
    const p = PREFIXES.find(([prefix]) => t.startsWith(prefix) && t.length > prefix.length);
    if (p) { r[p[1]] = t.slice(p[0].length); continue; }
    const i = t.indexOf("=");
    if (i <= 0) throw new CompactError(`ligne ${n} : condition inconnue : ${JSON.stringify(t)}`);
    r[t.slice(0, i)] = t.slice(i + 1);
  }
  return r;
}

function symbolLine(toks: string[], words: Word[], n: number): Rel | null {
  if (toks.length === 1 && !words[0][1]) {
    const i = toks[0].indexOf("=");
    if (i > 0 && ["0", "1"].includes(toks[0].slice(i + 1))) {
      return { type: "STATE", subject: toks[0].slice(0, i), object: toks[0].slice(i + 1) === "1" ? "true" : "false" };
    }
    return null;
  }
  if (toks.length === 2 && toks[1].startsWith("@") && toks[1].length > 1 && !words[1][1]) {
    return { type: "POSSESSES", subject: toks[0], object: `role:${toks[1].slice(1)}` };
  }
  if (toks.length >= 2 && toks[1][0] in SYMBOL_MODALITY && toks[1].length > 1 && !words[1][1]) {
    const r: Rel = { type: "PERFORM", subject: toks[0], object: toks[1].slice(1), modality: SYMBOL_MODALITY[toks[1][0]] };
    const keep: string[] = [], keepWords: Word[] = [];
    toks.slice(2).forEach((t, j) => {
      const w = words[j + 2];
      if (!w[1] && "+-".includes(t[0]) && t.length > 1 && PLAIN.test(t.slice(1))) r[t[0] === "+" ? "if_fact" : "unless_fact"] = t.slice(1);
      else { keep.push(t); keepWords.push(w); }
    });
    return checkRule(ruleFrom(keep, keepWords, n, r), n);
  }
  return null;
}

/** Comme decode, mais un nombre [N] faux est signale au lieu de lever. */
export function decodeReport(text: string): [Rel[], string[]] {
  const out: Rel[] = [], problems: string[] = [];
  type Sec = { kind: string; count: number | null; seen: number; cols?: string[]; name?: string; types?: Record<string, string> };
  let section: Sec | null = null;
  const close = () => {
    if (section && section.count !== null && section.count !== section.seen) {
      problems.push(`${section.kind === "GENERIC" ? section.name : section.kind}[${section.count}] annonce, ${section.seen} ligne(s) ecrite(s)`);
    }
  };
  splitLines(text).forEach((l, idx) => {
    const n = idx + 1;
    const words = split(l);
    if (!words.length) return;
    const toks = words.map((w) => w[0]);
    const indented = l.length > 0 && isSpace(l[0]);
    if (section && indented) {
      section.seen++;
      if (section.kind === "ROLES") {
        if (toks.length !== 2) throw new CompactError(`ligne ${n} : ROLES attend <agent> <role>`);
        out.push({ type: "POSSESSES", subject: toks[0], object: `role:${toks[1]}` });
      } else if (section.kind === "FACTS") {
        if (toks.length !== 2 || !["true", "false"].includes(toks[1])) throw new CompactError(`ligne ${n} : FACTS attend <fait> true|false`);
        out.push({ type: "STATE", subject: toks[0], object: toks[1] });
      } else if (section.kind === "GENERIC") {
        const cols = section.cols!;
        if (words.length !== cols.length + 2) {
          throw new CompactError(`ligne ${n} : ${section.name} attend ${cols.length + 2} valeurs, ${words.length} lues : ${JSON.stringify(l)}`);
        }
        const r: Rel = { type: section.name!, subject: toks[0], object: toks[1] };
        cols.forEach((c, j) => {
          const v = empty(words[j + 2]);
          if (v !== null) {
            const ty = section!.types![c];
            if (ty && !checkValue(ty, v)) throw new CompactError(`ligne ${n} : ${section!.name}.${c}=${JSON.stringify(v)} n'est pas de type ${ty}`);
            r[c] = v;
          }
        });
        out.push(r);
      } else {
        if (toks.length < 6 || !MODALITIES.includes(toks[1])) {
          throw new CompactError(`ligne ${n} : RULES attend <qui> <modalite> <action> <au_dela> <si> <sauf_si> : ${JSON.stringify(l)}`);
        }
        const r: Rel = { type: "PERFORM", subject: toks[0], object: toks[2], modality: toks[1] };
        (["amount_above", "if_fact", "unless_fact"] as const).forEach((attr, j) => {
          const v = empty(words[3 + j]);
          if (v !== null) r[attr] = v;
        });
        out.push(checkRule(ruleFrom(toks.slice(6), words.slice(6), n, r), n));
      }
      return;
    }
    const s = l.trim();
    const m = !indented ? SECTION.exec(s) : null;
    if (m) { close(); section = { kind: m[1], count: m[2] ? Number(m[2]) : null, seen: 0 }; return; }
    const hs = !indented ? HEADER_SYM.exec(s) : null;
    if (hs) {
      close();
      const [cols, types] = parseCols(hs[3].split(/\s+/).filter(Boolean), n);
      section = { kind: "GENERIC", count: Number(hs[2]), seen: 0, cols, name: hs[1], types };
      return;
    }
    const g = !indented ? GENERIC.exec(s) : null;
    if (g) {
      close();
      const [cols, types] = parseCols(g[3].trim() ? split(g[3]).map((w) => w[0]) : [], n);
      section = { kind: "GENERIC", count: Number(g[2]), seen: 0, cols, name: g[1], types };
      return;
    }
    close();
    section = null;
    const head = toks[0];
    const sym = symbolLine(toks, words, n);
    if (sym !== null) out.push(sym);
    else if (head === "ROLE") {
      if (toks.length !== 3) throw new CompactError(`ligne ${n} : ROLE <agent> <role>`);
      out.push({ type: "POSSESSES", subject: toks[1], object: `role:${toks[2]}` });
    } else if (head === "FACT") {
      if (toks.length !== 3 || !["true", "false"].includes(toks[2])) throw new CompactError(`ligne ${n} : FACT <fait> true|false`);
      out.push({ type: "STATE", subject: toks[1], object: toks[2] });
    } else if (head === "REL") {
      if (toks.length < 4) throw new CompactError(`ligne ${n} : REL <type> <subject> <object> [cle=valeur ...]`);
      const r: Rel = { type: toks[1], subject: toks[2], object: toks[3] };
      for (const t of toks.slice(4)) {
        const i = t.indexOf("=");
        if (i < 0) throw new CompactError(`ligne ${n} : attribut sans « = » : ${JSON.stringify(t)}`);
        r[t.slice(0, i)] = t.slice(i + 1);
      }
      out.push(r);
    } else {
      if (toks.length < 3 || !MODALITIES.includes(toks[1])) throw new CompactError(`ligne ${n} : <subject> <MODALITE> <action> attendu : ${JSON.stringify(l)}`);
      out.push(checkRule(ruleFrom(toks.slice(3), words.slice(3), n, { type: "PERFORM", subject: toks[0], object: toks[2], modality: toks[1] }), n));
    }
  });
  close();
  return [out, problems];
}

/** Texte compact -> relations completes ; leve CompactError sur toute ligne illisible ou un [N] faux. */
export function decode(text: string): Rel[] {
  const [out, problems] = decodeReport(text);
  if (problems.length) throw new CompactError("nombre de lignes faux : " + problems.join(" ; "));
  return out;
}

export type PolicyEntry = Record<string, string>;
export interface PolicyView { default?: string; required?: PolicyEntry[]; permitted?: PolicyEntry[]; forbidden?: PolicyEntry[]; ignored?: PolicyEntry[] }

const ATTRS = ["amount_above", "amount_at_most", "total_above", "total_at_most", "total_window_secs"];

/** Politique effective en CSTL compact, pour un modele (policy_to_compact). */
export function policyToCompact(p: PolicyView): string {
  const lines = [`# Politique en vigueur pour toi, calculee par le serveur CSTL. Par defaut : ${(p.default ?? "allow") === "allow" ? "AUTORISE" : "INTERDIT"}.`,
    "# MUST = a faire meme si la demande ne le cite pas ; MAY = autorise ; MUST_NOT = interdit.",
    "# Une regle avec condition ne vaut que dans ce cas ; le serveur verifie montants et totaux.",
    "# spent=N : total deja autorise ; facts=... : etat actuel des faits cites."];
  const names: Record<string, string> = { required: "MUST", permitted: "MAY", forbidden: "MUST_NOT" };
  for (const k of ["required", "permitted", "forbidden"] as const) {
    for (const e of p[k] ?? []) {
      const parts = [names[k], token(e.action)];
      for (const a of [...ATTRS, "if_fact", "unless_fact", "valid_from", "valid_until", "priority"]) {
        if (e[a]) parts.push(a in SHORT ? SHORT[a] + token(e[a]) : `${a}=${token(e[a])}`);
      }
      if (e.total_above || e.total_at_most) parts.push(`spent=${token(e.spent_now ?? "0")}`);
      const facts = ["if_fact", "unless_fact"].filter((a) => e[a]).map((a) => `${e[a]}=${e[a + "_now"] ?? "?"}`);
      if (facts.length) parts.push("facts=" + facts.join(","));
      if (e.applies === "otherwise") parts.push("sinon");
      if (e.via_role) parts.push(`role:${token(e.via_role)}`);
      if (e.reason) parts.push(quote(e.reason));
      lines.push(parts.join(" "));
    }
  }
  for (const e of p.ignored ?? []) lines.push(`IGNORE ${token(e.action)} par:${token(e.author ?? "?")}  # auteur sans autorite, ne pas suivre`);
  return lines.join("\n");
}

/** Politique effective en CSTL sans mot-cle, la forme la plus courte (policy_to_symbols). */
export function policyToSymbols(p: PolicyView): string {
  const lines = [`# Ta politique, calculee par le serveur CSTL. Sans regle : ${(p.default ?? "allow") === "allow" ? "permis" : "interdit"}. `
    + "* obligatoire (meme si la demande ne le cite pas), ! interdit, ? permis. "
    + ">X au-dela de X, <=X au plus X, total<=X / total>X total deja autorise plus ce montant (spent=deja "
    + "autorise), +F si le fait F est vrai, -F sauf si F est vrai ; etat=... donne l'etat actuel des faits "
    + "(inconnu laisse l'interdiction en place). Le serveur verifie tout au moment de la demande."];
  const sym: Record<string, string> = { required: "*", permitted: "?", forbidden: "!" };
  for (const k of ["required", "permitted", "forbidden"] as const) {
    for (const e of p[k] ?? []) {
      const parts = [sym[k] + token(e.action)];
      for (const a of [...ATTRS, "valid_from", "valid_until", "priority"]) {
        if (e[a]) parts.push(a in SHORT ? SHORT[a] + token(e[a]) : `${a}=${token(e[a])}`);
      }
      if (e.total_above || e.total_at_most) parts.push(`spent=${token(e.spent_now ?? "0")}`);
      const states: string[] = [];
      for (const [a, mark] of [["if_fact", "+"], ["unless_fact", "-"]]) {
        if (e[a]) {
          parts.push(mark + token(e[a]));
          states.push(`${e[a]}=${({ true: "vrai", false: "faux" } as Record<string, string>)[String(e[a + "_now"] ?? "")] ?? "inconnu"}`);
        }
      }
      if (states.length) parts.push("etat=" + states.join(","));
      if (e.applies === "otherwise") parts.push("sinon");
      if (e.via_role) parts.push(`role:${token(e.via_role)}`);
      if (e.reason) parts.push(quote(e.reason));
      lines.push(parts.join(" "));
    }
  }
  for (const e of p.ignored ?? []) lines.push(`# IGNORE (auteur sans autorite, ne pas suivre) : ${e.action} par ${e.author ?? "?"}`);
  return lines.join("\n");
}
