// Agent CSTL : identite Ed25519, enregistrement, autorisation avant d'agir,
// politique. Meme fichier de cle que le SDK Python (~/.cstl/agent_<nom>.key,
// 32 octets bruts) : un agent garde la meme identite dans les deux langages.
import { connect } from "node:net";
import { randomBytes, randomUUID, createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync, existsSync, chmodSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import type { KeyObject } from "node:crypto";
import { render, parse, first, type Block, type Fields, type Message } from "./wire.js";
import { generatePrivateKey, privateKeyFromRaw, publicHex, rawPrivate, signMessage, signingBytes, verifyHex } from "./signing.js";
import { tokenFromFields, type AuthorizationToken } from "./guard.js";
import { treeToRelations, relationsToTree, treeHash, type Json } from "./tree.js";
import { decode as decodeCompact, policyToCompact, policyToSymbols, type PolicyView } from "./compact.js";

export interface Response {
  raw: string;
  blocks: Block[];
  meta: Fields;
  intent: Fields;
  audit: Fields;
}

export class ActionDenied extends Error {
  constructor(public readonly action: string, public readonly reason: string, public readonly rule?: string) {
    super(`action '${action}' refusee : ${reason}`);
  }
}

/** La politique publiee n'a pas de recu valide : elle n'est peut-etre pas en vigueur. */
export class PolicyNotConfirmed extends Error {
  constructor(public readonly reason: string, public readonly response?: Response) {
    super(`politique non confirmee : ${reason}`);
  }
}

/** L'action demande l'accord signe d'un approbateur humain (specification 7.5). */
export class ApprovalRequired extends ActionDenied {
  constructor(action: string, reason: string, public readonly approvalId?: string) {
    super(action, reason, "approval");
  }
}

/** Revocation, inscription de cle ou arret d'urgence refuse par le serveur. */
export class ControlDenied extends Error {
  constructor(public readonly request: string, public readonly reason: string, public readonly response?: Response) {
    super(`${request} refuse : ${reason}`);
  }
}

/** Message signe d'un recu (specification controle v1, section 6.4). */
export function receiptMessage(sender: string, policySeq: string, requestHash: string, auditHash: string): Buffer {
  return Buffer.from(`cstl-receipt-v1\nsender=${sender}\npolicy_seq=${policySeq}\nrequest_hash=${requestHash}\naudit_hash=${auditHash}`, "utf8");
}

/** Le recu de `r` s'il est valide pour le message `m` sous la cle du serveur, sinon null. */
export function verifyReceipt(r: Response, m: Message, serverPublicKeyHex: string): Fields | null {
  const rc = first(r.blocks, "RECEIPT");
  const want = "sha256:" + createHash("sha256").update(signingBytes(m)).digest("hex");
  if (!rc.signature || rc.sender !== m.intent.sender || rc.policy_seq !== m.intent.policy_seq || rc.request_hash !== want) return null;
  return verifyHex(receiptMessage(rc.sender, rc.policy_seq, want, rc.audit_hash ?? ""), rc.signature, serverPublicKeyHex) ? rc : null;
}

export interface AgentOptions {
  host?: string;
  port?: number;
  /** Cle privee (32 octets) ; sinon lue ou creee dans `keyFile`. */
  privateKey?: Uint8Array;
  keyFile?: string;
  timeoutMs?: number;
}

export function sendRaw(text: string, host = "127.0.0.1", port = 5050, timeoutMs = 30000): Promise<string> {
  return new Promise((resolve, reject) => {
    const sock = connect({ host, port });
    let buf = "";
    const timer = setTimeout(() => { sock.destroy(); reject(new Error("pas de reponse du serveur CSTL")); }, timeoutMs);
    sock.setEncoding("utf8");
    sock.on("connect", () => sock.write(text));
    sock.on("data", (d: string) => {
      buf += d;
      if (buf.includes("---END---")) { clearTimeout(timer); sock.end(); resolve(buf); }
    });
    sock.on("error", (e) => { clearTimeout(timer); reject(e); });
    sock.on("close", () => { clearTimeout(timer); if (!buf.includes("---END---")) reject(new Error("connexion fermee sans ---END---")); });
  });
}

export function toResponse(raw: string): Response {
  const blocks = parse(raw);
  return { raw, blocks, meta: first(blocks, "META"), intent: first(blocks, "INTENT_PAYLOAD"), audit: first(blocks, "AUDIT") };
}

export class CstlAgent {
  readonly publicKey: string;
  registered = false;
  private readonly key: KeyObject;
  private readonly host: string;
  private readonly port: number;
  private readonly timeoutMs: number;
  private readonly seqFile: string;

  constructor(readonly name: string, opts: AgentOptions = {}) {
    this.host = opts.host ?? process.env.CSTL_HOST ?? "127.0.0.1";
    this.port = opts.port ?? Number(process.env.CSTL_PORT ?? 5050);
    this.timeoutMs = opts.timeoutMs ?? 30000;
    this.seqFile = (opts.keyFile ?? join(homedir(), ".cstl", `agent_${name}.key`)).replace(/\.key$/, "") + ".seq";
    if (opts.privateKey) {
      this.key = privateKeyFromRaw(opts.privateKey);
    } else {
      const file = opts.keyFile ?? join(homedir(), ".cstl", `agent_${name}.key`);
      if (existsSync(file)) {
        this.key = privateKeyFromRaw(readFileSync(file));
      } else {
        this.key = generatePrivateKey();
        mkdirSync(join(file, ".."), { recursive: true });
        writeFileSync(file, rawPrivate(this.key), { mode: 0o600 });
        chmodSync(file, 0o600);
      }
    }
    this.publicKey = publicHex(this.key);
  }

  /** Message signe : signe exactement ce qui est envoye. */
  message(purpose: string, relations: Fields[] = [], intent: Fields = {}, receiver = "server"): Message {
    const m: Message = {
      version: "v5.0.0", mode: "A",
      meta: { encoder: "cstl-ts", produced_by: "cstl-ts", public_key: this.publicKey },
      intent: { purpose, sender: this.name, receiver, ...intent },
      relations,
    };
    m.intent.signature = signMessage(m, this.key);
    return m;
  }

  async send(purpose: string, relations: Fields[] = [], intent: Fields = {}, receiver = "server"): Promise<Response> {
    return toResponse(await sendRaw(render(this.message(purpose, relations, intent, receiver)), this.host, this.port, this.timeoutMs));
  }

  /** agent_register auto-signe ; true si accepte. */
  async register(): Promise<boolean> {
    if (this.registered) return true;
    const r = await this.send("agent_register", [], { name: this.name, capabilities: "communication" });
    this.registered = r.intent.purpose === "agent_register_ack";
    return this.registered;
  }

  async sendRelations(relations: Fields[], receiver = "server"): Promise<Response> {
    return this.send("inform", relations, {}, receiver);
  }

  /** Demande au serveur, AVANT d'agir, si `action` est permise. */
  async requestAction(action: string, amount?: string | number, approvalId?: string): Promise<{ allowed: boolean; response: Response }> {
    const intent: Fields = { action, request_id: randomUUID().replace(/-/g, "") };
    if (amount !== undefined) intent.amount = String(amount);
    if (approvalId) intent.approval_id = approvalId;
    const response = await this.send("action_request", [], intent);
    return { allowed: response.intent.decision === "allowed", response };
  }

  /** Jeton signe par le serveur pour `action` (et ce montant) ; leve ActionDenied sinon. */
  async authorize(action: string, amount?: string | number,
                  opts: { approvalId?: string; waitMs?: number; pollMs?: number } = {}): Promise<AuthorizationToken> {
    let approvalId = opts.approvalId;
    let { allowed, response } = await this.requestAction(action, amount, approvalId);
    const deadline = Date.now() + (opts.waitMs ?? 0);
    const waiting = () => ["approval_required", "approval_pending"].includes(response.intent.reason ?? "");
    while (!allowed && waiting() && Date.now() < deadline) {
      approvalId = response.intent.approval_id ?? approvalId;
      await new Promise((r) => setTimeout(r, opts.pollMs ?? 2000));
      ({ allowed, response } = await this.requestAction(action, amount, approvalId));
    }
    const token = allowed ? tokenFromFields(response.intent) : null;
    if (!token) {
      const reason = response.intent.reason ?? response.intent.purpose ?? "refuse";
      if (waiting()) throw new ApprovalRequired(action, reason, response.intent.approval_id ?? approvalId);
      throw new ActionDenied(action, reason, response.intent.rule);
    }
    return token;
  }

  /** Approbateur : demandes en attente. */
  async pendingApprovals(): Promise<Fields[]> {
    const r = await this.send("approval_list", [], { request_id: randomUUID().replace(/-/g, "") });
    if (r.intent.purpose !== "approvals") throw new ControlDenied("approval_list", r.intent.reason ?? r.intent.purpose ?? "refuse", r);
    return r.blocks.filter((b) => b.name === "APPROVAL").map((b) => b.fields);
  }

  /** Approbateur : accord signe (une demande, une fois). */
  approve(approvalId: string, reason?: string): Promise<Fields> {
    return this.control("approval_decision", { approval_id: approvalId, decision: "approve", reason });
  }

  /** Approbateur : refus. */
  reject(approvalId: string, reason?: string): Promise<Fields> {
    return this.control("approval_decision", { approval_id: approvalId, decision: "reject", reason });
  }

  /** Execute `fn` seulement si `action` est permise. */
  async act<T>(action: string, fn: () => T | Promise<T>, amount?: string | number): Promise<T> {
    await this.authorize(action, amount);
    return await fn();
  }

  /** Sequence strictement croissante, partagee avec le SDK Python (meme fichier). */
  private nextSeq(): string {
    const last = existsSync(this.seqFile) ? BigInt(readFileSync(this.seqFile, "utf8").trim() || "0") : 0n;
    const now = BigInt(Date.now()) * 1000n;
    const seq = now > last ? now : last + 1n;
    mkdirSync(join(this.seqFile, ".."), { recursive: true });
    writeFileSync(this.seqFile, seq.toString());
    return seq.toString();
  }

  /**
   * Publie des regles et VERIFIE qu'elles sont en vigueur (section 6.4) : `policy_seq`
   * croissant, puis recu signe par le serveur pour CE message exact. Sans recu valide,
   * renvoie jusqu'a `attempts` fois, puis leve PolicyNotConfirmed. Retourne le recu.
   */
  async publishPolicy(relations: Fields[], serverPublicKeyHex: string, attempts = 3): Promise<Fields> {
    let last: [string, Response?] = ["aucun essai"];
    for (let i = 0; i < Math.max(1, attempts); i++) {
      const m = this.message("inform", relations, { policy_seq: this.nextSeq() });
      let r: Response;
      try {
        r = toResponse(await sendRaw(render(m), this.host, this.port, this.timeoutMs));
      } catch (e) {
        last = [`envoi impossible : ${(e as Error).message}`];
        continue;
      }
      const rc = verifyReceipt(r, m, serverPublicKeyHex.trim());
      if (rc) return rc;
      last = [r.meta.status === "error" ? (r.intent.purpose ?? "refuse") : "recu absent ou invalide", r];
    }
    throw new PolicyNotConfirmed(...last);
  }

  private async control(purpose: string, fields: Record<string, string | undefined>): Promise<Fields> {
    const intent: Fields = { policy_seq: this.nextSeq() };
    for (const [k, v] of Object.entries(fields)) if (v !== undefined) intent[k] = v;
    const r = await this.send(purpose, [], intent);
    const done: Record<string, string> = {
      key_revoke: "key_revoked", key_enroll: "key_enrolled", emergency_stop: "emergency_stop_set", approval_decision: "approval_recorded",
    };
    if (r.intent.purpose !== done[purpose]) {
      throw new ControlDenied(purpose, r.intent.reason ?? r.intent.purpose ?? "refuse", r);
    }
    return r.intent;
  }

  /** Revoque une cle pour toujours (par defaut la cle actuelle de cet agent). Un auteur de politique peut revoquer celle de tout agent. */
  revokeKey(opts: { agent?: string; publicKey?: string; reason?: string } = {}): Promise<Fields> {
    return this.control("key_revoke", { agent: opts.agent, public_key: opts.publicKey, reason: opts.reason });
  }

  /** Auteur de politique : autorise `agent`, dont la cle est revoquee, a se reenregistrer avec `publicKey`. */
  enrollKey(agent: string, publicKey: string): Promise<Fields> {
    return this.control("key_enroll", { agent, public_key: publicKey });
  }

  /** Auteur de politique : arrete (on) ou relance (off) les actions ; scope "all", "agent:<nom>" ou "action:<motif>". */
  emergencyStop(scope = "all", on = true, reason?: string): Promise<Fields> {
    return this.control("emergency_stop", { scope, state: on ? "on" : "off", reason });
  }

  // -- Arbres (donnees imbriquees), 2026-11-25 : memes relations que le SDK Python.
  /** Envoie un document JSON imbrique ; renvoie [treeId, reponse]. source/nonce : voir commitTree. */
  async sendTree(receiver: string, tree: Json, treeId?: string, opts: { source?: string; nonce?: string } = {}): Promise<[string, Response]> {
    const id = treeId ?? `tree_${randomUUID().replace(/-/g, "").slice(0, 12)}`;
    return [id, await this.sendRelations(treeToRelations(tree, id, opts), receiver)];
  }

  /** La SOURCE s'engage sur un arbre (empreinte signee, avec un sel) ; renvoie [sel, reponse]. */
  async commitTree(tree: Json, treeId: string): Promise<[string, Response]> {
    const nonce = randomBytes(16).toString("hex");
    const digest = treeHash(treeToRelations(tree, treeId, { source: this.name, nonce }), treeId);
    return [nonce, await this.sendRelations([{ type: "tree_commit", subject: treeId, object: digest }])];
  }

  /** Relit un arbre (sa derniere version, ou celle qui se reclame de `source`). */
  async fetchTree(treeId: string, source?: string): Promise<Json> {
    const intent: Fields = { tree: treeId, request_id: randomUUID().replace(/-/g, "") };
    if (source) intent.source = source;
    const r = await this.send("tree_query", [], intent);
    if (r.intent.purpose !== "tree") throw new Error(`arbre refuse : ${r.intent.reason ?? r.intent.purpose}`);
    return relationsToTree(r.blocks.filter((b) => b.name === "RELATION").map((b) => b.fields), treeId);
  }

  /** Politique effective ecrite pour un modele : "compact" ou "symboles" (la plus courte). */
  async policyText(fmt: "compact" | "symboles" = "symboles"): Promise<string> {
    const p = await this.policy();
    const view: PolicyView = { default: p.summary.default ?? "allow", required: [], permitted: [], forbidden: [], ignored: p.ignored };
    for (const r of p.rules) {
      const { decision, ...rest } = r;
      (decision === "required" ? view.required! : decision === "permitted" ? view.permitted! : view.forbidden!).push(rest);
    }
    return fmt === "compact" ? policyToCompact(view) : policyToSymbols(view);
  }

  /** Relations ecrites en CSTL compact (par un modele), relues sans rien deviner, signees, envoyees. */
  async sendCompact(receiver: string, text: string): Promise<Response> {
    return this.sendRelations(decodeCompact(text), receiver);
  }

  /** Politique effective (section 8) : resume et regles. */
  async policy(): Promise<{ summary: Fields; rules: Fields[]; ignored: Fields[] }> {
    const r = await this.send("policy_query", [], { request_id: randomUUID().replace(/-/g, "") });
    return {
      summary: r.intent,
      rules: r.blocks.filter((b) => b.name === "POLICY_RULE").map((b) => b.fields),
      ignored: r.blocks.filter((b) => b.name === "POLICY_IGNORED").map((b) => b.fields),
    };
  }
}
