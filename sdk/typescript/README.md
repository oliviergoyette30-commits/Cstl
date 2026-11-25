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
  `requestAction()`, `sendRelations()`, `policy()`.
- `CstlGuard.run` refuse :
  - un jeton absent ou falsifié ;
  - un jeton pour une autre action, un autre agent ou un autre montant ;
  - un jeton expiré, ou déjà utilisé.
- Même fichier de clé que le SDK Python (`~/.cstl/agent_<nom>.key`, 32
  octets) : un agent garde la même identité dans les deux langages.

Tests : `npm test`. Il construit le SDK, vérifie les vecteurs de
conformité, puis lance les tests en direct contre `target/debug/cstl_parser`
(`cargo build --bin cstl_parser`).
