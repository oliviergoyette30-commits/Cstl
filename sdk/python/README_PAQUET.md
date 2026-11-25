# cstl — client Python du serveur CSTL

Contrôle des agents IA : un agent demande l'autorisation **avant** d'agir,
reçoit un jeton signé par le serveur, et le système qui exécute vérifie ce
jeton lui-même. Politique, plafonds de montant, audit infalsifiable.

```bash
pip install ./sdk/python                     # depuis le depot
cargo build --release --bin cstl_parser      # le serveur (Rust)
cstl-demo                                    # la demonstration, ~2 s
```

Commandes installées :

| Commande | Rôle |
|---|---|
| `cstl-demo` | démonstration complète contre le vrai serveur |
| `cstl-mcp-proxy -- <serveur MCP>` | CSTL devant n'importe quel serveur MCP |
| `cstl-mcp-server` | les outils CSTL pour un client MCP |
| `cstl-witness` | témoin externe de l'audit (retour en arrière) |

Modules : `cstl_llm_agent` (`CstlAgent`), `cstl_guard` (`CstlGuard`),
`cstl_signing`, `cstl_compact`, `cstl_formes`, `cstl_client`.
Documentation complète : `AGENTS.md` à la racine du dépôt.
