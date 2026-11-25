// Agent CSTL : identite Ed25519, enregistrement, autorisation avant d'agir,
// politique. Meme fichier de cle que le SDK Python (~/.cstl/agent_<nom>.key,
// 32 octets bruts) : un agent garde la meme identite dans les deux langages.
import { connect } from "node:net";
import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync, existsSync, chmodSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import type { KeyObject } from "node:crypto";
import { render, parse, first, type Block, type Fields, type Message } from "./wire.js";
import { generatePrivateKey, privateKeyFromRaw, publicHex, rawPrivate, signMessage } from "./signing.js";
import { tokenFromFields, type AuthorizationToken } from "./guard.js";

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

  constructor(readonly name: string, opts: AgentOptions = {}) {
    this.host = opts.host ?? process.env.CSTL_HOST ?? "127.0.0.1";
    this.port = opts.port ?? Number(process.env.CSTL_PORT ?? 5050);
    this.timeoutMs = opts.timeoutMs ?? 30000;
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
  async requestAction(action: string, amount?: string | number): Promise<{ allowed: boolean; response: Response }> {
    const intent: Fields = { action, request_id: randomUUID().replace(/-/g, "") };
    if (amount !== undefined) intent.amount = String(amount);
    const response = await this.send("action_request", [], intent);
    return { allowed: response.intent.decision === "allowed", response };
  }

  /** Jeton signe par le serveur pour `action` (et ce montant) ; leve ActionDenied sinon. */
  async authorize(action: string, amount?: string | number): Promise<AuthorizationToken> {
    const { allowed, response } = await this.requestAction(action, amount);
    const token = allowed ? tokenFromFields(response.intent) : null;
    if (!token) throw new ActionDenied(action, response.intent.reason ?? response.intent.purpose ?? "refuse", response.intent.rule);
    return token;
  }

  /** Execute `fn` seulement si `action` est permise. */
  async act<T>(action: string, fn: () => T | Promise<T>, amount?: string | number): Promise<T> {
    await this.authorize(action, amount);
    return await fn();
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
