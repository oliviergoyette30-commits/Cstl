# CSTL comme connecteur dans claude.ai

Ce guide branche CSTL dans claude.ai (web et mobile) comme **connecteur
personnalisé**. Claude y gagne des outils pour lire sa politique et
demander une autorisation signée avant d'agir :
- `cstl_policy` : ses obligations, permissions et interdictions ;
- `cstl_authorize` : la décision, et un jeton signé si l'action est permise ;
- `cstl_send_compact`, `cstl_send_relations`, `cstl_compact_guide`.

Pour Claude Desktop et Claude Code, un connecteur local suffit (README,
section « Get started », et `cstl mcp-server`). Ce guide sert pour
claude.ai, qui n'appelle que des serveurs **distants en HTTPS**.

## Depuis un téléphone seulement : Render, sans terminal

Tout se fait dans le navigateur du téléphone. Le dépôt contient
`render.yaml`, que Render lit pour tout créer, et `Dockerfile.cloud`, un
seul conteneur qui fait tourner le serveur, la politique et le connecteur.

1. **Créer le service.** Va sur **render.com**, connecte-toi avec ton compte
   GitHub, puis choisis **New → Blueprint** et le dépôt `Cstl`. Render lit
   `render.yaml` et propose le service `cstl` : valide avec **Apply**.
2. **Attendre la construction.** Elle prend environ 10 minutes, car Render
   compile le serveur.
3. **Récupérer les deux informations :**
   - l'adresse, en haut de la page du service :
     `https://cstl-xxxx.onrender.com` ;
   - le jeton, dans l'onglet **Environment**, valeur de `CSTL_MCP_TOKEN`
     (bouton pour l'afficher).
4. **Ajouter le connecteur dans claude.ai** (navigateur du téléphone) :
   **Paramètres → Connecteurs → Ajouter un connecteur personnalisé**,
   avec l'URL `https://cstl-xxxx.onrender.com/mcp/<le jeton>`.
5. **Changer les règles.** Elles sont dans **Environment**, variable
   `CSTL_POLICY`, une règle par ligne. Après une modification, Render
   redémarre le service et republie la politique.
6. **Tester** comme à la section 6.

Ce qu'il faut savoir sur l'offre gratuite de Render :
- **Le service s'endort** après 15 minutes sans usage. Le premier appel
  suivant prend environ une minute.
- **Rien n'est gardé.** Sans disque persistant (payant), la base, les clés
  et l'audit repartent de zéro à chaque redémarrage. C'est bien pour
  essayer, pas pour un vrai usage.
- **La construction n'a pas été essayée chez Render.** Elle a été vérifiée
  avec Docker dans l'environnement de développement. Si elle échoue chez
  Render (mémoire, temps), envoie le message d'erreur.

## 1. Ce qu'il faut
- Le dépôt CSTL, avec Docker, ou avec Rust et Python 3.10 ou plus.
- Une façon de publier un port en HTTPS sur internet :
  - un tunnel (Cloudflare Tunnel), depuis ta machine ;
  - ou un serveur avec un nom de domaine et Caddy.
- Un abonnement claude.ai qui permet d'ajouter des connecteurs
  personnalisés.

## 2. Démarrer CSTL et le connecteur

Le connecteur exige un **jeton secret**. Il fait partie de l'adresse que tu
donneras à claude.ai : garde-le comme un mot de passe.

Avec Docker :

```bash
export CSTL_MCP_TOKEN=$(openssl rand -hex 32)
echo $CSTL_MCP_TOKEN                       # note-le
docker compose --profile claude up -d      # le serveur CSTL + le connecteur sur 127.0.0.1:8765
```

Sans Docker :

```bash
cstl init && cstl serve                    # terminal 1
export CSTL_MCP_TOKEN=$(openssl rand -hex 32)
CSTL_AGENT=agent_claude cstl mcp-remote --listen 127.0.0.1:8765   # terminal 2
```

Le connecteur agit au nom de **un** agent CSTL : `agent_claude` par défaut
(`CSTL_MCP_AGENT` avec Docker, `CSTL_AGENT` sans).

## 3. Le publier en HTTPS

**Le plus rapide, pour essayer :** un tunnel Cloudflare, sans compte.
L'adresse change à chaque lancement.

```bash
cloudflared tunnel --url http://127.0.0.1:8765
# affiche une adresse https://xxxx.trycloudflare.com
```

**Pour durer :** un serveur avec un nom de domaine et Caddy, qui obtient le
certificat tout seul. Le `Caddyfile` :

```
mcp.ton-domaine.com {
    reverse_proxy 127.0.0.1:8765
}
```

Le serveur CSTL lui-même reste sur 127.0.0.1 : seul le connecteur est
publié.

## 4. L'ajouter dans claude.ai

1. Ouvre **Paramètres → Connecteurs → Ajouter un connecteur personnalisé**.
2. Nom : `CSTL`.
3. URL : `https://<ton-adresse>/mcp/<ton-jeton>`.
4. Laisse les réglages OAuth vides.
5. Dans une conversation, active le connecteur CSTL depuis le menu des
   outils.

## 5. Écrire la politique de Claude

```
agent_claude MAY read_issues
agent_claude MUST_NOT delete_repo "irreversible"
agent_claude MUST_NOT send_payment >50 "above 50: a human decides"
```

```bash
cstl policy apply policy.cstl                                   # sans Docker
docker compose exec -T cstl cstl policy apply - < policy.cstl   # avec Docker
```

## 6. Tester

Dans claude.ai, demande :
- « Lis ta politique avec cstl_policy. »
- « Demande à cstl_authorize si tu peux faire delete_repo. » Réponse
  attendue : refusé, avec la règle `MUST_NOT`.
- « Et send_payment pour 30 ? » Réponse attendue : permis, avec un jeton
  signé.

Chaque demande est signée et inscrite dans l'audit du serveur. Le tableau de
bord (`/v1/dashboard`, sur la machine du serveur) les montre.

## 7. Ce que ça protège, et ce que ça ne protège pas

- **Claude demande avant d'agir, et chaque demande est auditée.** Le
  serveur lui dit aussi d'appeler `cstl_authorize` avant toute action qui a
  un effet.
- **Mais une demande reste volontaire.** CSTL n'empêche pas claude.ai
  d'utiliser ses autres connecteurs sans demander. Pour qu'une action soit
  **impossible** sans autorisation, le système qui l'exécute doit exiger le
  jeton :
  - `cstl-gate`, pour SQL, shell et HTTP ;
  - `@guarded`, dans ton propre serveur MCP ;
  - `A2AGuard`, dans ton propre agent A2A.
  
  Avec ces gardes, un connecteur claude.ai qui passe par eux ne peut rien
  faire d'interdit.
- **L'adresse donne l'accès.** Il n'y a pas d'OAuth : qui connaît l'adresse
  avec le jeton peut parler au nom de `agent_claude`. Si elle fuit, change le
  jeton (redémarre avec un nouveau `CSTL_MCP_TOKEN`), puis corrige l'adresse
  dans claude.ai.
- Une requête de navigateur venant d'un autre site que claude.ai ou
  claude.com est refusée.
- Le jeton n'est jamais écrit dans le journal du connecteur.

Test en direct : `sdk/python/test_mcp_remote_live.py`. Il a été vérifié par
HTTP, comme le ferait un client MCP. La connexion depuis claude.ai lui-même
n'a pas pu être essayée depuis l'environnement de développement.

## 8. La démonstration du vrai contrôle (essayée le 2026-11-27)

Avec `CSTL_DEMO_TOOLS=1`, déjà mis dans `render.yaml`, le connecteur ajoute
trois outils. Ils agissent sur de faux dépôts et de faux paiements :
- `demo_list_repos` : l'état réel, sans contrôle ;
- `demo_delete_repo` : exige un jeton signé pour `delete_repo` ;
- `demo_send_payment` : exige un jeton signé pour `send_payment` et ce
  montant.

Ces outils exigent le jeton **eux-mêmes**, que Claude ait demandé
l'autorisation ou non.

Ce qui s'est passé, en vrai, dans claude.ai sur un téléphone, avec le
serveur sur Render :

| Demande à Claude | Ce que Claude a fait | Résultat |
|---|---|---|
| Supprimer `prod-db` | a demandé, s'est vu refuser, s'est arrêté | rien d'exécuté |
| Envoyer 30 à alice | a demandé, autorisé | **exécuté**, l'outil a vérifié le jeton |
| Envoyer 200 à bob | a demandé, s'est vu refuser | rien d'exécuté |
| Appeler quand même `demo_delete_repo("prod-db")` et `demo_send_payment(200)`, **sans demander** | a appelé les outils directement | **refusés par les outils eux-mêmes**, rien d'exécuté |
| État final (`demo_list_repos`) | | `prod-db` existe toujours ; un seul paiement, 30 à alice |

C'est la différence entre une **demande** et un **contrôle**. Un agent qui
coopère demande et respecte la réponse. Un outil gardé refuse même un
agent qui ne demande pas, qui se trompe ou qui a été manipulé.

Pour un vrai usage, retire `CSTL_DEMO_TOOLS`.

## 9. Une vraie action : les issues GitHub

Le connecteur peut agir sur **un** dépôt GitHub, avec quatre outils :
- `github_list_issues` : en lecture libre ;
- `github_create_issue`, `github_comment` et `github_close_issue` : chacun
  protégé par `@guarded`.

Le jeton GitHub reste sur le serveur : Claude ne le voit jamais, et ne peut
écrire sur GitHub **que** par ces outils.

1. **Créer un jeton GitHub limité.** Sur github.com : **Settings →
   Developer settings → Personal access tokens → Fine-grained tokens →
   Generate new token**.
   - Repository access : **Only select repositories**, et un seul dépôt
     (de préférence un dépôt d'essai).
   - Permissions : **Issues** en **Read and write**. Rien d'autre.
2. **Dans Render :** service `cstl` → **Environment**. Remplis :
   - `CSTL_GITHUB_TOKEN` : le jeton ;
   - `CSTL_GITHUB_REPO` : `propriétaire/dépôt`.
   
   Puis **Save**. Render redémarre le service.
3. **Dans claude.ai :** le connecteur passe de 8 à 12 outils.
4. **Tester**, avec la politique d'exemple (`MAY github_create_issue`,
   `MUST_NOT github_close_issue`) :
   - « Crée une issue "Essai CSTL" avec github_create_issue » : l'issue
     apparaît sur GitHub ;
   - « Ferme l'issue 1 avec github_close_issue » : refusé, et GitHub n'est
     pas appelé.

Test en direct : `test_github_tools_reach_github_only_with_a_signed_token`
(dans `sdk/python/test_mcp_remote_live.py`), contre une fausse API GitHub
qui note chaque appel.

**Essayé contre le vrai GitHub le 2026-11-27.** Le serveur tournait sur
Render, et le dépôt d'essai était `cstl-essai`, avec un jeton à grain fin
limité aux issues de ce dépôt :

| Action | Règle | Résultat |
|---|---|---|
| `github_create_issue` « Essai CSTL » | `MAY` | **exécuté** : issue #1 créée sur GitHub |
| `github_comment` sur la #1, depuis claude.ai sur un téléphone | `MAY` | **exécuté** : commentaire ajouté |
| `github_close_issue` sur la #1, appelé directement, sans `cstl_authorize` | `MUST_NOT` | **refusé**, GitHub n'a pas été appelé |
| `github_list_issues` ensuite | lecture | la #1 est toujours ouverte |

Le jeton GitHub n'a jamais quitté le serveur.

## 10. Avant un vrai usage (revue d'attaque du 2026-11-27)

La revue complète est dans `docs/RED_TEAM_2026-11-27.md`. Aucune attaque n'a
permis une action interdite. Mais ce déploiement d'essai a deux limites à
lever avant un vrai usage :

- **L'audit doit survivre.** Sur l'offre gratuite de Render, la base et
  l'audit repartent de zéro à chaque redémarrage. Il faut :
  - un disque persistant monté sur `/data` ;
  - de préférence le témoin externe (`cstl-witness`), sur une autre machine.
- **La clé de l'auteur ne doit pas être sur le serveur.** En mode cloud,
  `CSTL_POLICY` fait signer la politique par une clé créée dans le même
  conteneur que le connecteur. Pour un vrai usage, l'auteur publie depuis
  sa propre machine (`cstl policy apply`) vers un serveur CSTL que lui seul
  peut joindre.

Les réglages par défaut sont maintenant plus prudents :
- outils de démonstration coupés (`CSTL_DEMO_TOOLS=0`) ;
- outils d'écriture de relations coupés (`CSTL_MCP_SEND_TOOLS=1` pour les
  activer) ;
- au plus 600 requêtes et 60 ouvertures de session par minute.
