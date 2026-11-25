// Verification d'un jeton d'autorisation par le systeme qui execute
// (specification controle v1, section 7.3). Equivalent de cstl_guard.CstlGuard.
import { verifyHex, publicKeyFromHex } from "./signing.js";
import { connect } from "node:net";
import { randomBytes } from "node:crypto";

export const AUTHZ_VERSION = "cstl-authz-v1";

export interface AuthorizationToken {
  agent: string;
  action: string;
  request_id: string;
  issued_at: number;
  expires_at: number;
  audit_hash: string;
  signature: string;
  amount?: string | null;
}

export type Verdict =
  | "no_token" | "bad_signature" | "wrong_action" | "wrong_agent" | "wrong_amount"
  | "not_yet_valid" | "expired" | "already_used"
  // Verification en direct (specification 7.6) :
  | "revoked" | "stopped" | "unknown" | "status_unavailable";

export class Unauthorized extends Error {
  constructor(public readonly reason: Verdict) {
    super(`action refusee : ${reason}`);
  }
}

export function tokenMessage(t: AuthorizationToken): Buffer {
  let text = `${AUTHZ_VERSION}\nagent=${t.agent}\naction=${t.action}\nrequest_id=${t.request_id}\n` +
    `issued_at=${t.issued_at}\nexpires_at=${t.expires_at}\naudit_hash=${t.audit_hash}`;
  if (t.amount !== undefined && t.amount !== null) text += `\namount=${t.amount}`;
  return Buffer.from(text, "utf8");
}

/** Jeton d'une reponse action_authorization (champs INTENT_PAYLOAD), ou null. */
export function tokenFromFields(f: Record<string, string>): AuthorizationToken | null {
  if (f.decision !== "allowed") return null;
  const need = ["receiver", "action", "request_id", "issued_at", "expires_at", "audit_hash", "authorization"];
  if (need.some((k) => f[k] === undefined)) return null;
  const issued = Number(f.issued_at), expires = Number(f.expires_at);
  if (!Number.isInteger(issued) || !Number.isInteger(expires)) return null;
  return { agent: f.receiver, action: f.action, request_id: f.request_id, issued_at: issued, expires_at: expires,
           audit_hash: f.audit_hash, signature: f.authorization, amount: f.amount ?? null };
}

function num(x: string | number | null | undefined): number | null {
  if (x === null || x === undefined) return null;
  const s = String(x).trim();
  if (s === "") return null;
  const n = Number(s);
  return Number.isFinite(n) ? n : null;
}

export interface GuardOptions {
  skew?: number;
  clock?: () => number; // secondes unix
  /** Serveur CSTL a interroger avant d'executer (purpose=token_status). */
  online?: { host: string; port: number; timeoutMs?: number };
}

export class CstlGuard {
  private readonly key: string;
  private readonly skew: number;
  private readonly clock: () => number;
  private readonly used = new Set<string>();
  private readonly online?: { host: string; port: number; timeoutMs?: number };

  /** `serverPublicKeyHex` : cle publique du serveur, obtenue HORS BANDE (<base>.server_key.pub). */
  constructor(serverPublicKeyHex: string, opts: GuardOptions = {}) {
    publicKeyFromHex(serverPublicKeyHex);
    this.key = serverPublicKeyHex.trim();
    this.skew = opts.skew ?? 5;
    this.clock = opts.clock ?? (() => Date.now() / 1000);
    this.online = opts.online;
  }

  /** Etat du jeton selon le serveur, en direct ; "status_unavailable" sans reponse signee. */
  async liveStatus(t: AuthorizationToken): Promise<Verdict | "valid"> {
    if (!this.online) return "valid";
    const nonce = randomBytes(16).toString("hex");
    const text = "#!CSTL v5.0.0 MODE=A\nMETA [encoder=cstl_guard, produced_by=cstl_guard]\n" +
      `INTENT_PAYLOAD [purpose=token_status, sender=cstl_guard, receiver=server, audit_hash=${t.audit_hash}, nonce=${nonce}]\n---END---\n`;
    const { host, port, timeoutMs = 5000 } = this.online;
    const raw = await new Promise<string | null>((resolve) => {
      let buf = "";
      const s = connect({ host, port }, () => s.write(text));
      s.setTimeout(timeoutMs, () => { s.destroy(); resolve(null); });
      s.on("data", (d) => { buf += d.toString("utf8"); if (buf.includes("---END---")) { s.end(); resolve(buf); } });
      s.on("error", () => resolve(null));
      s.on("close", () => resolve(buf.includes("---END---") ? buf : null));
    });
    if (!raw) return "status_unavailable";
    const line = raw.split("\n").find((l) => l.startsWith("INTENT_PAYLOAD ["));
    if (!line) return "status_unavailable";
    const f: Record<string, string> = {};
    for (const part of line.slice("INTENT_PAYLOAD [".length).replace(/\]$/, "").split(", ")) {
      const i = part.indexOf("=");
      if (i > 0) f[part.slice(0, i).trim()] = part.slice(i + 1).trim();
    }
    if (f.purpose !== "token_status" || f.nonce !== nonce || f.audit_hash !== t.audit_hash) return "status_unavailable";
    const msg = Buffer.from(`cstl-token-status-v1\naudit_hash=${t.audit_hash}\nstatus=${f.status}\nnonce=${nonce}\nchecked_at=${f.checked_at}`, "utf8");
    if (!verifyHex(msg, f.signature ?? "", this.key)) return "status_unavailable";
    return (["valid", "expired", "revoked", "stopped", "unknown"] as const).find((v) => v === f.status) ?? "status_unavailable";
  }

  /** Leve Unauthorized si le jeton ne permet pas `action` maintenant ; ne le consomme pas. */
  check(t: AuthorizationToken | null | undefined, action: string, agent?: string | null, amount?: string | number | null): void {
    if (!t) throw new Unauthorized("no_token");
    if (!verifyHex(tokenMessage(t), t.signature, this.key)) throw new Unauthorized("bad_signature");
    if (t.action !== action) throw new Unauthorized("wrong_action");
    if (agent !== undefined && agent !== null && t.agent !== agent) throw new Unauthorized("wrong_agent");
    const tokenHas = t.amount !== undefined && t.amount !== null;
    const execHas = amount !== undefined && amount !== null;
    if (tokenHas || execHas) {
      const a = num(amount), b = num(t.amount);
      if (a === null || b === null || a !== b) throw new Unauthorized("wrong_amount");
    }
    const now = this.clock();
    if (t.issued_at > now + this.skew) throw new Unauthorized("not_yet_valid");
    if (now > t.expires_at + this.skew) throw new Unauthorized("expired");
  }

  /** Execute `fn` SEULEMENT avec un jeton valide jamais utilise ; le jeton est consomme avant. */
  async run<T>(t: AuthorizationToken | null | undefined, action: string, fn: () => T | Promise<T>,
               opts: { agent?: string; amount?: string | number } = {}): Promise<T> {
    this.check(t, action, opts.agent, opts.amount);
    if (this.online) {
      const status = await this.liveStatus(t!);
      if (status !== "valid") throw new Unauthorized(status);
    }
    if (this.used.has(t!.request_id)) throw new Unauthorized("already_used");
    this.used.add(t!.request_id);
    return await fn();
  }
}
