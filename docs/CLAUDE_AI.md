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
