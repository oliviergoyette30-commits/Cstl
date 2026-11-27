# cstl (TypeScript)

Client du serveur CSTL pour Node.js (20 et plus), **sans aucune dépendance** :
Ed25519, TCP et HTTP viennent de Node. Il implémente la spécification
`docs/CSTL_CONTROLE_SPEC_v1.md` et donne exactement ses vecteurs de
conformité, comme les SDK Python et Rust.

```ts
import { CstlAgent, CstlGuard, ActionDenied } from "cstl";

const bot = new CstlAgent("agent_support", { port: 5050 });   // cle dans ~/.cstl/agent_agent_support.key
await bot.register();

// Demander AVANT d'agir : jeton signe par le serveur, ou ActionDenied.
const token = await bot.authorize("refund_order", 80);

// Dans le systeme qui execute : il ne croit que la cle publique du serveur.
const guard = new CstlGuard(serverPublicKeyHex);          // <base>.server_key.pub, hors bande
await guard.run(token, "refund_order", () => refund(80), { agent: "agent_support", amount: 80 });
```

- `CstlAgent` : `register()`, `authorize(action, amount?)`, `act(action, fn)`,
  `requestAction()`, `sendRelations()`, `policy()`, et
  `publishPolicy(relations, cleServeur)`. Cette dernière envoie avec
  `policy_seq`, vérifie le reçu signé par le serveur pour ce message exact,
  et lève `PolicyNotConfirmed` sans reçu valide (spécification, section 6.4).
- `CstlGuard.run` refuse :
  - un jeton absent ou falsifié ;
  - un jeton pour une autre action, un autre agent ou un autre montant ;
  - un jeton expiré, ou déjà utilisé.
- **Contrôle (2026-11-25) :**
  - `revokeKey`, `enrollKey` : révocation d'une clé volée (section 5.5) ;
  - `emergencyStop(scope, on)` : arrêt d'urgence (section 7.4) ;
  - `authorize(action, amount, { approvalId, waitMs })`, `pendingApprovals`,
    `approve`, `reject` : accord humain (section 7.5), avec `ApprovalRequired` ;
  - `new CstlGuard(cle, { online: { host, port } })` : vérifie en direct que
    le jeton vaut encore, ni révoqué ni arrêté (section 7.6).
- **Données :**
  - `sendTree`, `fetchTree`, `commitTree` : arbres JSON imbriqués, avec les
    mêmes relations et la même empreinte que le SDK Python (vecteurs dans
    `test/tree_vectors.json`) ;
  - `policyText("symboles" | "compact")`, `sendCompact(dest, texte)` et
    `encode`, `encodeSymbols`, `decode` : le CSTL compact, identique au
    Python sur 900 encodages (`test/compact_vectors.json`). Les arbres
    indentés et les formes de documents ne sont qu'en Python.
- Même fichier de clé que le SDK Python (`~/.cstl/agent_<nom>.key`, 32
  octets) : un agent garde la même identité dans les deux langages.

Tests : `npm test`. Il construit le SDK, vérifie les vecteurs de
conformité, puis lance les tests en direct contre `target/debug/cstl_parser`
(`cargo build --bin cstl_parser`).
