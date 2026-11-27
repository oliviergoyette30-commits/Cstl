# CSTL — ce qui manque (feuille de route, 2026-11-21)

Point de départ honnête :

- **Un protocole ne crée pas d'intelligence.** CSTL ne rapproche pas de la
  super-intelligence. Au mieux, il permet à beaucoup d'agents de coopérer
  de façon fiable et de rester contrôlables.
- **Ce qui aide, d'après les mesures, c'est le serveur.**
  `scripts/team_task_benchmark.py` le montre : politique effective,
  autorisations signées et historique infalsifiable. Le format du langage,
  lui, n'apporte rien de mesurable. Le JSON libre fait presque aussi bien
  (45/48 et 48/48).

Ce qui manquerait pour que CSTL soit le meilleur protocole entre IA, du
plus important au moins important :

1. **Parler les protocoles existants.** Les agents parlent déjà MCP
   (outils) et A2A (agent à agent). CSTL deviendrait la couche de contrôle
   par-dessus : autorisation, politique, audit, vérification. Il faut des
   adaptateurs MCP et A2A.
2. **Prouver qu'il est meilleur.** Un banc d'essai indépendant :
   délégation, négociation, planification, chaînes de plus de 10 agents,
   plusieurs fournisseurs de modèles, et des cas écrits par d'autres.
3. **Exprimer ce que les agents s'échangent vraiment :**
   - tâches et délégation (qui fait quoi, pour quand, état d'avancement,
     qui a validé) ;
   - preuves attachées (source, calcul, empreinte des données) ;
   - propositions imbriquées (« A croit que B a fait X ») ;
   - quantificateurs (« pour tout client ») ;
   - négociation (offre, contre-offre, accord).
4. **Vérifier les faits, pas seulement la cohérence :** affirmations
   signées par leur source, résultats de calcul vérifiables, confiance
   calculée sur l'historique des affirmations confirmées.
5. **Une mémoire partagée vérifiée :** une base de faits commune aux
   agents, interrogeable, avec la provenance de chaque fait.
6. **L'échelle :** plusieurs serveurs qui se font confiance entre
   organisations, des identités standard, la révocation des clés
   compromises, et pas seulement une base SQLite.
7. **Le coût :** mesurer les tokens face au JSON. Un format compact pour
   une machine peut coûter plus cher à comprendre pour un modèle.
8. **L'adoption :** une spécification courte et stable, des SDK Python et
   TypeScript, un paquet installable, une démo de 10 minutes, des tests de
   conformité.
9. **Des agents très capables :**
   - contrôle obligatoire au niveau de l'infrastructure (le garde
     `cstl_guard.py` est un début) ;
   - escalade vers un humain et arrêt d'urgence ;
   - limites de débit et détection des comportements anormaux.

Ordre proposé : 1, puis 2, puis 3 et 4.

## Plan pour devenir la couche de contrôle de référence (2026-11-25)

La cible n'est pas « meilleur format que le JSON », mais la couche de
contrôle des agents : autorisation, politique, audit. Le format arbre est un
bonus sur les données tabulaires. Sur les données irrégulières, CSTL strict
bat le JSON avec une consigne courte (48/48 contre 36 et 42/48) et fait jeu
égal avec 3 exemples (46 contre 47).

1. **Utilisable par n'importe qui :**
   - **fait le 2026-11-25 :**
     - entrée JSON `POST /v1/message` ;
     - paquet Python `cstl` (roue testée dans un environnement vierge) ;
     - démo `cstl-demo` (7 vérifications, environ 2 s) ;
     - workflow des binaires Linux, macOS et Windows (pas encore lancé sur
       GitHub) ;
     - spécification v1 figée (`docs/CSTL_CONTROLE_SPEC_v1.md`), vecteurs
       vérifiés en Python et en Rust, testeur de serveur boîte noire (16/16).
       En l'écrivant, trouvé et corrigé : `verify_authorization` (Rust) ne
       vérifiait pas le montant ;
   - à faire : publication (PyPI, release).
2. **Brancher les écosystèmes :**
   - **fait le 2026-11-25 :** proxy MCP devant tout serveur MCP
     (`cstl_mcp_proxy.py`, fermé par défaut, jetons vérifiés) ;
   - **fait le 2026-11-25 :** proxy A2A 1.0 (`cstl_a2a.py`) : jeton CSTL
     vérifié pour chaque appel JSON-RPC, appelant vérifié transmis à l'agent
     cible, carte d'agent republiée ;
   - **fait le 2026-11-25 :** SDK TypeScript (`sdk/typescript`, aucune
     dépendance) : vecteurs de conformité identiques, tests en direct, même
     fichier de clé que Python ;
   - **fait le 2026-11-25 :** `cstl-gate`, la garde qui exécute des
     opérations nommées (SQL SQLite et PostgreSQL, shell sans interpréteur,
     HTTP aux secrets de la garde), jeton vérifié et paramètres typés avant
     toute exécution ; testé avec un vrai PostgreSQL.
3. **Prouver publiquement :**
   - **commencé le 2026-11-25 :** banc public déterministe
     (`benchmark/control_benchmark.py`, 12 attaques en fichiers JSON
     ouverts aux contributions, vrai serveur). Première mesure : en T2
     (canal de l'auteur compromis), CSTL 7 cas compromis contre 1 pour une
     politique signée et versionnée : ordre de réception, et pas de
     rattrapage. Corrigé le même jour (`policy_seq` et reçu signé,
     spécification 6.4, `publish_policy`) : CSTL seul à 0 violation et 0
     refus à tort dans les deux modèles de menace ;
   - **fait :** `publishPolicy` dans le SDK TypeScript ;
   - **fait :** banc avec modèles (GPT-5 mini, Llama 4, Claude Haiku,
     Mistral ; 3 passes ; écart-type ≤ 1). CSTL et la politique signée
     versionnée font jeu égal (24/24 avec GPT-5 mini et Haiku). Les garde-fous
     qui jugent le message reçu perdent. Les 6 cas compromis de CSTL viennent
     d'une politique de l'auteur absente ou contradictoire (refusée par le
     serveur), avec un serveur ouvert par défaut ;
   - **mesuré :** le mode fermé (`CSTL_ACTION_DEFAULT=deny`) supprime les
     cas compromis sur tous les modèles (Llama : CSTL 13/0 contre 10/0 ;
     Mistral : 15/0 contre 17/0), mais fait 5 refus à tort dans le banc
     déterministe. Aucun défaut ne gagne partout. Le défaut reste ouvert ;
     le mode fermé est recommandé avec `CSTL_POLICY_AUTHORS` pour les
     actions à risque ;
   - **fait :** actions gouvernées (`CSTL_GOVERNED_ACTIONS`) : banc
     déterministe 0 violation / 0 refus à tort sous T1 et T2 ; banc avec
     modèles (Haiku, GPT-5 mini ; 5 passes) : 0 cas compromis, égalité
     statistique avec la politique signée versionnée (56 contre 58 sur 59,
     p = 0,50). Llama et Mistral restent à mesurer (crédits épuisés) ;
   - banc reproductible contre OPA/Cedar, NeMo, Llama Guard et JSON signé,
     3 passes au moins ;
   - questions plus dures ;
   - attaques écrites par d'autres ;
   - audit de sécurité externe.
4. **Échelle :** PostgreSQL (le serveur reste sur SQLite).
   - **fait (2026-11-25) :** fédération entre serveurs par attestations
     signées, à durée courte (spécification 5.8,
     `test_federation_live.py`) ;
   - **fait (2026-11-25) :** identités standard, OIDC à l'enregistrement
     (RS256, ES256) et SPIFFE en TLS mutuel. Spécification 5.6 et 5.7, tests
     `test_oidc_live.py` et `test_spiffe_live.py` ;
   - **fait (2026-11-25) :** accord humain (`CSTL_APPROVAL_ACTIONS`,
     `cstl-approve`), vérification des jetons en direct (`token_status`),
     arrêt automatique sur comportement anormal
     (`CSTL_ANOMALY_MAX_PER_MIN`), tableau de bord (`/v1/dashboard`).
     Spécification 7.5 à 7.7, test `sdk/python/test_approval_live.py` ;
   - **fait (2026-11-25) :** révocation des clés compromises (`key_revoke`,
     `key_enroll`) et arrêt d'urgence (`emergency_stop`, plus le fichier
     d'arrêt de l'opérateur). Spécification sections 5.5 et 7.4, test
     `sdk/python/test_revocation_live.py`, conformance C17 et C18.
5. **Adoption :** nom définitif, article, 3 utilisateurs pilotes. Le format
   compact viendra quand un modèle sera entraîné sur CSTL.

## Faiblesses connues (évaluation du 2026-11-25)

Mesurées ou constatées dans ce dépôt ; à corriger ou à assumer.

**Format**
- Sur des données irrégulières, aucune forme CSTL compacte ne bat le JSON en
  écriture : `mixte` 37/48, `relationnel` 40 à 44/48, JSON 36 à 47/48.
  Seule la forme `strict` est plus fiable (48/48 à 1 exemple), et elle coûte
  +7 à +18 % de tokens.
- `strict` ressemble à un sous-ensemble du YAML : ce n'est pas une
  invention de CSTL, seulement un choix de forme sûre.
- Corrigé le 2026-11-25 : dans `strict`, la valeur inconnue s'écrit `?` ou
  `?{"sigma":0.3}`, comme dans l'arbre indenté.
- `relationnel` n'est pas plus court que le JSON selon l'estimateur local
  (−11 à +39 %), seulement −6 à 0 % selon les tokenizers des fournisseurs.
- Le tour de correction répare peu : 8 erreurs détectées sur 16 à
  1 exemple, aucune à 3 exemples. Les modèles refont la même erreur.
- Un tiers environ des erreurs restantes sont silencieuses (champ inventé,
  liste vide ajoutée) : aucun lecteur ne les voit.
- Beaucoup de formes (relations 4, documents 5, spécialisées 8). Le choix
  automatique les cache, mais c'est plus à maintenir qu'un seul format.
- Aucun modèle n'est entraîné sur CSTL : les formes compactes resteront
  moins fiables en écriture tant que ce n'est pas le cas.

**Mesures**
- Souvent une seule passe, avec un bruit de ±2 à 6 points (le JSON a fait
  42/48 puis 36/48 sur le même test).
- Questions de lecture trop faciles : beaucoup de 36/36 partout, effet
  plafond. Il faut des questions plus dures et des arbres de profondeur 5
  et plus.
- Six formes spécialisées (série, graphe, conversation, appels, matrice,
  changements) sont sans perte mais jamais mesurées sur un modèle.
- Le passage de DeepSeek sur les formes de relations est perdu (contenu
  vide de l'API).

**Sécurité**
- Les comptes d'un arbre (`tree_n`) prouvent qu'il correspond à ce que
  l'auteur annonce, pas que l'auteur dit vrai.
- Un arbre envoyé sans `source` n'est garanti que par son expéditeur.
- Le témoin externe reçoit les maillons de l'audit en asynchrone : ceux
  écrits juste avant un arrêt brutal peuvent lui manquer.
- L'autorisation reste volontaire sans garde (`CstlGuard`) dans le système
  qui exécute l'action.

**Produit**
- Pas de paquet installable, pas de SDK TypeScript, pas d'adaptateur A2A
  (MCP seulement).
- Le serveur reste sur SQLite (pas de PostgreSQL). La fédération entre
  serveurs, OIDC et SPIFFE existent depuis le 2026-11-25.
- Sans vérification en direct (`CstlGuard(online=...)` ou
  `CSTL_GUARD_ONLINE`), un jeton déjà délivré reste valable jusqu'à son
  expiration (60 s par défaut), même après une révocation ou un arrêt
  d'urgence.
- Adoption nulle : c'est un prototype de recherche, pas encore un produit.

## Mesures

- **`scripts/team_task_benchmark.py` :** une tâche d'équipe avec pièges.
  Avec la politique effective, CSTL fait 48/48 avec Haiku 5.5 et
  Opus 5.5. Le JSON libre fait 45/48 et 48/48.
- **`scripts/relay_tamper_benchmark.py` :** une politique relayée par une
  chaîne d'agents dont l'un est compromis. CSTL fait 48/48, avec 0 cas
  compromis. En texte, il y a 7 cas compromis sur 48, et en JSON 15 sur
  48.
  - Les attaques qui passent toujours en texte et en JSON : la limite de
    montant relevée, le faux accord (en JSON) et l'obligation supprimée.
  - L'avantage vient de l'architecture : une source signée et un serveur
    qui fait foi. Le langage n'y est pour rien.

- **`scripts/policy_history_benchmark.py` :** appliquer l'état final après
  un long historique de révisions (jusqu'à 60 événements). Texte 24/24,
  JSON 21/24 et 24/24, CSTL 24/24.
  - Ce n'est pas un point faible des modèles actuels, donc CSTL n'y
    apporte pas d'avantage net.
  - Le test a révélé et fait corriger quatre défauts de CSTL : la
    permission MAY manquait, la politique ne disait ni la règle par défaut
    ni l'état des faits, elle montrait des règles remplacées, et le serveur
    revérifiait toutes les signatures à chaque demande (192 ms à
    600 règles).

- **`scripts/budget_benchmark.py` :** un plafond par remboursement et un
  budget cumulé, sur 20 à 60 demandes, avec 3 répétitions.
  - CSTL fait 36/36 avec les deux modèles, et 0 euro dépensé en trop,
    puisque c'est le serveur qui additionne.
  - Le JSON fait aussi 36/36.
  - Le texte libre fait 35/36, avec un cas de dépassement par modèle.

- **`scripts/competition_benchmark.py` :** CSTL contre les garde-fous du
  marché, face au même relais compromis (3 passes × 8 scénarios).
  - CSTL : 24/24 avec Haiku 5.5 et Opus 5.5, 0 cas compromis.
  - Garde LLM (façon NeMo ou Llama Guard) : 6 et 7 cas compromis.
  - Moteur de règles (façon OPA ou Cedar) : 9 et 8 cas compromis.
  - Ces deux approches jugent le message reçu, donc une limite relevée ou
    un faux consentement passent à chaque fois.
  - JSON signé : 0 cas compromis, mais tout est refusé quand le message est
    falsifié. Résultat : 15 obligations oubliées et 9/24 parfaits.
  - L'avantage de CSTL vient du serveur qui garde la copie signée.

- **`scripts/format_vs_server_benchmark.py` :** il sépare le format CSTL
  du serveur CSTL (2 formats × avec ou sans serveur).
  - Sans serveur, JSON et format CSTL laissent passer les mêmes attaques :
    6 cas compromis sur 24 pour chacun.
  - Avec le serveur, les deux formats font 0 cas compromis. JSON 24/24
    avec les deux modèles, CSTL 23/24 et 24/24.
  - Le format CSTL est environ 12 % plus court en tokens, mais pas plus
    fiable.
  - **Conclusion : l'avantage de CSTL face au JSON vient entièrement du
    serveur.** Le serveur peut servir des agents qui parlent JSON. C'est un
    argument de plus pour le point 1 : faire de CSTL une couche de contrôle
    par-dessus JSON, MCP et A2A.

- **`scripts/audit_benchmark.py` :** un initié falsifie le journal après
  coup. Six attaques sur le vrai fichier SQLite du serveur : montant
  modifié, entrée supprimée, fausse entrée, réécriture avec empreintes
  recalculées, fin coupée et retour à une ancienne copie.
  - CSTL les détecte toutes, sans fausse alerte. Il refuse de démarrer et
    dit pourquoi.
  - Un modèle qui lit un journal texte ou JSON ne voit avec certitude que
    l'entrée supprimée, à cause du trou de numérotation. Le reste, ce sont
    des intuitions, avec des fausses alertes : 3 sur 3 pour Opus en JSON.
  - La chaîne d'empreintes est trompée par un attaquant qui recalcule. Les
    signatures par entrée ne voient ni la fin coupée ni l'ancienne copie.
  - Un journal signé avec un ancrage externe ferait aussi bien. CSTL l'a
    déjà, intégré à l'autorisation.

- **`scripts/wire_size_benchmark.py` :** la taille, mesurée sur 16 vrais
  messages capturés sur le serveur (signatures, clés, jetons, audit
  compris). Le JSON porte exactement les mêmes informations, vérifié dans
  les deux sens.
  - Face au JSON compact, CSTL est 7 % plus petit en octets et 3 % en
    tokens, mais 3 % plus gros après gzip.
  - Face au JSON indenté, CSTL est 26 % plus petit en octets et 21 % en
    tokens.
  - Le gain est réel mais faible, et il disparaît une fois compressé.

- **`scripts/format_comprehension_benchmark.py` :** un modèle
  comprend-il aussi bien les trois formats ? La même politique de 10 à 60
  règles, avec rôles, faits et conditions, en CSTL, en JSON compact et en
  JSON indenté.
  - Aucune différence de compréhension entre les formats.
  - Opus fait tout juste dans les trois formats.
  - Haiku rate les obligations des grandes politiques, quel que soit le
    format : 18/36 en CSTL, 16/36 en JSON compact, 17/36 en JSON indenté.
  - C'est la taille de la politique qui fait échouer, pas le format, d'où
    l'intérêt que le serveur calcule la politique effective.

- **`scripts/compression_benchmark.py` :** toute la chaîne de compression
  de CSTL contre un JSON compressé avec les mêmes armes, sur les mêmes 16
  vrais messages.
  - Côté CSTL : Master hybride (dictionnaires réseau fixes, gzip
    structurel, zstd avec dictionnaire embarqué), dictionnaire de texte avec
    deltas, et réponses compressées du serveur.
  - Côté JSON : gzip, zstd, brotli, et zstd avec un dictionnaire entraîné.
  - Avec la meilleure méthode de chaque côté, la taille est la même :
    2 070 contre 2 076 octets. La méthode gagnante est la même pour les
    deux : zstd avec un dictionnaire entraîné.
  - Les compresseurs maison de CSTL font environ 5 fois moins bien
    (environ 10 000 octets).
  - Défaut trouvé puis corrigé : `master_decompress` ajoutait
    `object=""` aux relations qui n'en avaient pas. La correction garde le
    même format binaire, donc les données déjà stockées se lisent comme
    avant.
  - Piste : remplacer les compresseurs maison par zstd avec un
    dictionnaire entraîné, qui vaudrait aussi pour les agents JSON.

- **Banc public contre le vrai OPA** (2026-11-27, OPA 1.23.0,
  `benchmark/opa_runner.py`, `benchmark/results_opa.json`). Mêmes 12
  attaques ; paquets construits par `opa build`, signés RS256, vérifiés par
  le vrai `opa run`.
  - OPA sans signature : 10 cas compromis. OPA avec paquets signés : 4 cas
    compromis (4 violations, 2 refus à tort). CSTL : 0.
  - La signature d'OPA refuse bien un paquet modifié ou signé par une autre
    clé. Elle ne protège pas l'ordre : rejeu d'un vieux paquet signé,
    paquet supprimé, désordre.
  - Cedar n'est pas mesuré : c'est un évaluateur sans distribution ni
    vérification de la politique (rôle d'un service cloud).
  - Reste à faire : d'autres attaques écrites par d'autres que nous, et une
    passerelle MCP du marché.

- **`scripts/tree_compression_benchmark.py` : l'arbre indenté compressé**
  (2026-11-27, question d'Olivier). Le banc précédent ne mesurait que le
  format du fil. Ici, les données elles-mêmes, relues identiques, et chaque
  compression vérifiée.
  - Données imbriquées (12 documents) : l'arbre indenté reste plus petit
    que le JSON compact après tous les compresseurs : −12 % gzip, −10 %
    zstd, −10 % brotli, −15 % zstd avec dictionnaire entraîné (3 578 contre
    4 202 octets). Brut : −53 %.
  - Réponses d'API réelles (12) : −17 % brut, mais seulement −2 % après
    gzip, zstd ou brotli, et +4 % avec dictionnaire (entraîné sur 11
    documents très différents : point faible).
  - TOON et le JSON indenté sont plus gros que le JSON compact une fois
    compressés ; strict coûte 0 à 5 % de plus.
  - Conclusion : sur des données imbriquées et tabulaires, l'arbre indenté
    garde un avantage après compression. Ailleurs, la compression efface
    la différence.

- **`scripts/format_short_benchmark.py` :** un format abrégé qui n'écrit
  pas ce qui est prévisible (idée d'Olivier). Il n'envoie que l'imprévisible
  : qui, quoi, quelle règle, quel montant. Un programme reconstruit le
  message complet pour le serveur.
  - Le CSTL abrégé divise les tokens des données par 2,5 (782 contre 1 971
    pour le CSTL complet et 2 039 pour le JSON compact).
  - Avec la convention de lecture comprise, la consigne complète coûte
    39 % de moins.
  - Aucune perte de compréhension : Opus fait tout juste dans les quatre
    formats, et Haiku fait un peu mieux en abrégé (23/36 contre 17/36).
  - Le JSON abrégé coûte 32 % de plus que le CSTL abrégé.
  - C'est le premier avantage mesuré du format CSTL face au JSON.
  - À faire :
    - une vue abrégée de la politique effective dans le SDK, à la place de
      `policy_to_text` ;
    - un programme qui reconstruit le message complet à partir de la forme
      abrégée.

- **CSTL compact, module et tests (`sdk/python/cstl_compact.py`) :**
  conversion sans perte dans les deux sens (12 tests, dont 300 allers-retours
  aléatoires), et vue compacte de la politique effective.
  - **Écriture (`scripts/compact_writing_benchmark.py`) :** Opus écrit le
    compact sans aucune erreur (36/36), mieux que le CSTL complet (31/36) et
    le JSON (30/36). Haiku l'écrit moins bien (21/36 contre 33/36 en JSON) :
    il oublie des règles et confond if: et unless:. Le compact coûte 2,4 fois
    moins de tokens à écrire.
  - **Vue du serveur (relais compromis, budget) :** même fiabilité que les
    autres vues (24/24 et 0 cas compromis, 36/36 au budget). Mais sur une
    petite politique, elle n'est pas plus courte que les lignes CSTL
    complètes (291 tokens contre 250) : ses 4 lignes d'explication pèsent
    plus que les 2 ou 3 règles.
  - À faire :
    - raccourcir l'explication de la vue compacte ;
    - pour Haiku, éviter « sauf si » ou ajouter un exemple avec unless: ;
    - utiliser le compact pour les modèles forts et le JSON pour les petits
      modèles, tant que l'écart n'est pas comblé.

- **Comparaison avec TOON (`scripts/toon_lite.py`)** : encodeur fidèle à
  la spécification officielle (v4.4). TOON reçoit sa forme la plus
  favorable : des tableaux avec `null` pour les conditions absentes.
  - **Lecture :** le CSTL compact coûte 25 % de tokens de moins que TOON
    (782 contre 1 046), à compréhension égale. Opus fait 36/36 partout, et
    Haiku 24, 22 et 20 sur 36.
  - **Écriture avec Haiku :** TOON fait mieux que le CSTL compact (32/36
    contre 18/36) et que le JSON (30/36). Il coûte 1 045 tokens, contre 785
    pour le CSTL compact.
  - **Écriture avec Opus :** TOON n'a pas été mesuré (crédit épuisé). En
    CSTL compact, Opus faisait 36/36.
  - **Conclusion :** le CSTL compact est le plus court. TOON est plus facile
    à écrire pour un petit modèle, sans doute parce que ses colonnes sont
    nommées une fois dans l'en-tête.
  - Piste : un en-tête de colonnes dans le CSTL compact pour les petits
    modèles. **Fait (mode tableau, `encode(..., table=True)`)** :
    - une section par type d'élément, chacune avec son nombre de lignes et
      ses colonnes ;
    - des colonnes nommées en français (`au_dela si sauf_si`), et `-` pour
      une case vide ;
    - un nombre de lignes faux est refusé : une règle oubliée ne passe plus
      en silence ;
    - taille : 22 091 caractères sur les 12 politiques, contre 19 923 en
      lignes et 25 229 en TOON ;
    - **Mesuré (3 passes × 12 politiques, `scripts/tableau_*.json`) : le
      mode tableau n'aide pas.**

      | | Lecture Haiku | Lecture Opus | Écriture Haiku | Écriture Opus | Tokens lus / écrits |
      |---|---|---|---|---|---|
      | CSTL compact en lignes | 26/36 | 36/36 | 23/36 | 36/36 | 782 / 784 |
      | CSTL compact en tableaux | 15/36 | 36/36 | 24/36 (10 illisibles) | 35/36 | 873 / 875 |
      | TOON | 22/36 | 36/36 | 31/36 | 35/36 | 1 046 / 1 047 |

    - Haiku décale les colonnes : il écrit `- - - patient_consent`, trois
      tirets comme dans la ligne d'exemple qui finit par une raison, puis le
      fait dans une 7ᵉ colonne. Le nombre de lignes vérifié a bien repéré
      un oubli (`RULES[66]` annoncé, 60 lignes écrites).
    - Conclusion : le mode ligne reste le meilleur CSTL compact. Il est le
      plus court, et Opus l'écrit sans faute. Pour un petit modèle qui
      écrit, TOON reste le plus sûr.
    - Piste suivante, sans colonnes à compter : garder le mode ligne mais
      nommer les conditions en français (`si:F`, `sauf_si:F`) pour éviter la
      confusion if/unless de Haiku.

- **`scripts/data_types_benchmark.py` :** cinq autres types de données
  (tâches, faits, commandes, événements, négociation), 20 jeux de données
  et 180 questions.
  - **Hors politiques, TOON est le plus court.** Le CSTL compact coûte 33 %
    de tokens de plus (1 571 contre 1 185), parce qu'il répète le nom de
    chaque champ à chaque ligne.
  - Le CSTL compact reste 22 % plus court que le JSON compact.
  - La compréhension est la même dans tous les formats (174 à 179 sur 180).
  - Le CSTL compact n'est donc le plus court que pour les politiques, dont
    les conditions sont facultatives.
  - Piste : une section en tableau générique dans le CSTL compact, pour
    les données régulières, qui n'écrirait les noms de champs qu'une fois.

- **Sections en tableau génériques et mode sans mot-clé (`cstl_compact`)**
  : tout est prêt, mais rien n'est encore mesuré avec un modèle, car la
  limite de dépense de l'API est atteinte jusqu'au 1er décembre.
  - Le tableau générique écrit `ORDER[N] subject object amount ...` puis une
    ligne de valeurs par élément. Les sections s'inspirent de TOON.
  - Le mode sans mot-clé (`encode_symbols`) n'a plus aucun mot-clé :
    `agent_a @finance`, `kyc_done=1`, `agent_f !wipe_db >100 -audit_open`,
    et pour les données `ORDER[N]: amount ...`.
  - Taille hors ligne, en caractères :

    | | CSTL compact (lignes) | tableau | sans mot-clé | TOON | JSON |
    |---|---|---|---|---|---|
    | politiques (12) | 19 923 | 22 091 | **15 647** | 25 229 | – |
    | données (20 jeux) | 59 057 | 37 281 | **36 791** | 40 992 | 82 789 |

  - Mesures à lancer :
    - `format_short_benchmark.py --formats cstl_court,toon,cstl_symboles` ;
    - `compact_writing_benchmark.py --formats compact,toon,compact_symboles` ;
    - `data_types_benchmark.py --formats json,toon,cstl_tableau,cstl_symboles`.

- **`scripts/domains_benchmark.py` : les 18 domaines de `src/domains.rs`.**
  Chaque domaine a une politique de 15 règles sur ses propres opérateurs,
  plus 30 relations du domaine. La mesure est hors ligne, en caractères.
  - Le mode sans mot-clé est le plus court dans les 18 domaines : 23 % de
    moins que TOON et 53 % de moins que le JSON compact.
  - Même le mode ligne bat TOON ici (49 675 contre 52 920 caractères), car
    les données mélangent de nombreux types de relations.
  - Allers-retours sans perte partout.
  - Tokens et compréhension à mesurer quand l'API sera de nouveau
    disponible : `python3 scripts/domains_benchmark.py --tokens`.
  - Test de compréhension prêt (6 questions par domaine : 3 permissions,
    1 liste d'obligations, 1 comptage, 1 somme) :
    `python3 scripts/domains_benchmark.py --comprehension claude-haiku-5-5`,
    puis la même commande avec `claude-opus-5-5`.

- **Faits à trois états (couche 9, 2026-11-22)** : vrai, faux ou inconnu.
  Recommandation tirée de l'évaluation de ChatGPT.
  - Défaut corrigé : un fait jamais déclaré valait faux, donc « interdit de
    liquider s'il y a un incident » (`if_fact=incident_open`) était levé tant
    que personne n'avait déclaré l'état de l'incident.
  - Maintenant, un fait inconnu suit la règle de prudence : l'interdiction
    s'applique, l'obligation et la permission non.
  - La politique effective donne l'état de chaque fait cité :
    `if_fact_now` / `unless_fact_now` = `true`, `false` ou `unknown`.
  - Tests : test unitaire `unknown_facts_are_treated_with_prudence`, et test
    de bout en bout `test_unknown_fact_keeps_a_prohibition`.
- **Priorités et exceptions (couche 9, 2026-11-22).**
  - Avec `CSTL_POLICY_AUTHORS`, les règles sont classées par `priority`,
    puis par date. Une exception prioritaire l'emporte sur une règle
    générale, même plus récente, dans sa plage de montants.
  - En mode `Anyone`, la priorité est ignorée : sinon, n'importe quel agent
    pourrait lever une interdiction avec une priorité élevée.
- **Typage (2026-11-22).**
  - Sur le serveur : `RELATION [type=SCHEMA, subject=<type>,
    champ=num|int|date|bool|str]`. Une relation mal typée fait refuser tout
    le message (`schema_rejected`). Seuls comptent les schémas d'un auteur
    de politique.
  - Dans le CSTL compact : des colonnes typées (`amount:num`), reprises des
    déclarations `SCHEMA` ou données à l'encodeur, et vérifiées à l'encodage
    comme au décodage. Les conditions des règles sont typées aussi :
    montants, dates, durée et priorité.

- **`scripts/gemini_tokens.py` : les tokens comptés par le tokenizer de
  Gemini (2026-11-22).** C'est un deuxième tokenizer, après celui de Claude.
  - Le CSTL sans mot-clé est le plus court partout : 5 540 tokens sur les
    politiques, 20 743 sur les données, 20 284 sur les domaines.
  - TOON fait 9 788, 24 817 et 27 399 tokens. Le CSTL sans mot-clé fait
    donc 43 %, 16 % et 26 % de moins.
  - Le JSON compact fait 15 064, 32 358 et 30 968 tokens.

- **Point 1, premier pas : serveur MCP (`sdk/python/cstl_mcp_server.py`,
  2026-11-22).** N'importe quel agent qui parle MCP lit sa politique,
  demande une autorisation et reçoit un jeton signé, ou envoie des
  relations, sans apprendre CSTL. Le serveur passe par stdio en JSON-RPC,
  sans dépendance. Il est testé de bout en bout contre le vrai serveur.
  Reste à faire : A2A, et l'entrée JSON directe sur le serveur.

- **`scripts/nested_benchmark.py` : données imbriquées (2026-11-22).** Trois
  jeux : commandes → lignes → options, départements → équipes → membres,
  contrats → sections → clauses. Les tokens sont comptés avec Gemini.
  - Aplatir l'arbre en relations CSTL coûte plus que le JSON : 21 637 tokens
    contre 17 577 au mieux. C'est le point faible attendu, et il est
    confirmé.
  - Le nouveau mode arbre du CSTL compact (`encode_tree`) bat le JSON de
    17 % (14 524 tokens) et TOON de 31 %. Il est sans perte, vérifié par
    300 arbres aléatoires.
  - Compréhension, avec Gemma 4 31B sur une passe : JSON 32/33, TOON
    36/36, CSTL sans mot-clé 34/36, CSTL en arbre 32/36.
    - La plupart des erreurs portent sur une question : un filtre sur un
      montant absent, noté `-` en CSTL.
    - L'arbre est le plus court, mais un peu moins bien suivi. C'est à
      confirmer, sur une série courte et un seul modèle.
  - **Arbre indenté** (`encode_tree_indented`) : les enfants sont écrits
    sous leur parent, sans identifiant, et un champ facultatif n'est écrit
    que s'il existe (`note=fragile`). 10 792 tokens, soit 39 % de moins que
    le JSON et 26 % de moins que l'arbre par identifiants.
    - Compréhension (Gemma 4 31B, mêmes 36 questions) : arbre indenté
      **36/36**, JSON 35/36, TOON 35/36, arbre par identifiants 31/36.
    - Un seul modèle et une passe : à confirmer sur d'autres modèles.
    - Les trois limites relevées ensuite sont levées (2026-11-22) :
      - un texte multiligne s'écrit en bloc (`|N`, puis N lignes `| `) ;
      - les listes et objets s'écrivent en ligne (`tags=[a,b]`,
        `geo={lat:1}`) ;
      - la racine est libre (`@@ champs`, `@@list`, `@@= valeur`).
    - Allers-retours sans perte sur 30 000 documents JSON aléatoires,
      valeurs pièges comprises. Sur les 12 jeux du banc d'essai, la sortie
      est identique au token près.
    - Branché au serveur : `send_tree` accepte l'arbre indenté et n'importe
      quel document JSON. Une liste, un objet ou un texte multiligne part en
      JSON base64url (type `json`). Seules limites : 300 caractères par
      valeur et noms de champ `[A-Za-z0-9_.-]`.
  - **Écriture par un modèle** (`scripts/nested_writing_benchmark.py`,
    Gemma 4 31B, 12 jeux) :
    - JSON 10/10 identiques, TOON 8/10 (98 % des valeurs), arbre indenté
      9/12 en strict et **12/12 en mode tolérant**.
    - Tous les échecs de l'arbre indenté viennent d'un compte `[N]` faux sur
      les gros jeux. Le contenu, lui, est juste à 100 %.
    - Mettre le compte à la fin (`@end`) n'aide pas : 8/12 en strict, 12/12
      en tolérant. Le modèle compte aussi mal ce qu'il vient d'écrire.
    - Avec un compte **local** en fin de ligne (`/N`, le nombre d'enfants
      directs, `counts="line"`) : **12/12 identiques en mode strict**, tous
      les comptes justes. Un modèle compte bien de petits nombres qu'il vient
      d'écrire. Coût : +6 % de tokens (11 457), soit −35 % face au JSON.
    - Conclusion : pour un texte écrit par un modèle, le compte local garde
      la vérification stricte et indique la ligne fautive. Le mode tolérant
      (`lenient=True`) reste le filet de sécurité.
  - **Complétude vérifiée par le serveur** (2026-11-23) : chaque élément
    qui a des enfants porte `tree_n`, et `tree_root` porte le nombre
    d'éléments à la racine.
    - À la réception, le serveur refuse un arbre dont il manque une branche,
      ou qui a un élément sans parent ou en double (`purpose=tree_rejected`).
    - Les comptes sont signés et audités, puis revérifiés à la relecture.
    - Test réel : un client qui contourne le SDK est refusé.
    - Limite : les comptes sont ceux de l'auteur.
  - **Empreinte signée par la source** (2026-11-23) : la source s'engage
    (`commit_tree`, empreinte canonique SHA-256, signée et auditée).
    - Un arbre envoyé avec `source=<agent>` n'est accepté que s'il a
      exactement cette empreinte.
    - Test réel : un transmetteur qui omet une ligne, change un prix, ajoute
      une commande ou s'engage à la place de la source est refusé.
    - L'empreinte est la même en Python et en Rust (valeur figée des deux
      côtés).
  - **Valeurs longues** (2026-11-23) : découpées en morceaux de 250
    caractères (`tree_c<i>_<nom>`, type `nom:str/N`), ainsi que la liste des
    types. Test réel : un texte de 7 000 caractères et une liste de 500
    nombres passent par le serveur et reviennent identiques.
  - **Inconnu natif, le « vide qui parle »** (2026-11-23) : `?` ou
    `?{sigma:0.3,...}` dans l'arbre indenté, `{"$unknown": {...}}` en JSON.
    Il est distinct de `null` et d'un champ absent. Aller-retour sans perte
    (30 000 documents aléatoires), jusqu'au serveur.
    - Mesuré (`scripts/unknown_benchmark.py`, Gemma 4 31B, 24 questions) :
      arbre indenté 24/24 avec 2 390 tokens. JSON avec `$unknown` 24/24
      (3 697 tokens), TOON 24/24 (4 928 tokens).
    - Le JSON naïf, où l'inconnu devient `null`, fait 8/24 : 1/6, 0/6 et 1/6
      sur les questions de vide. L'information n'existe plus.
    - L'arbre indenté est le seul format complet et plus court que le JSON
      naïf (2 972 tokens).
    - Étendu à tous les formats courants, 24/24 partout sauf le JSON naïf :
      CSV 2 293 tokens (données plates, son terrain idéal), arbre indenté
      2 390, arbre par identifiants 2 665, JSON compact 3 697, YAML 4 802,
      TOON 4 928, XML 5 809, JSON indenté 7 604.
    - Ce qui compte est que le vide soit écrit, pas le format choisi. Entre
      formats complets, la différence est le coût.
  - **10 modèles, 5 fournisseurs** (2026-11-23, `scripts/multi_model_summary.py`).
    Modèles : GPT-5 mini, Claude Haiku et Sonnet 5.5, DeepSeek V4.1,
    Qwen 3.8, Llama 4 Maverick, Mistral Medium 3.5, Gemma 4 26B et 31B,
    Gemini 3.5 Flash-Lite.
    - Totaux JSON / TOON / arbre indenté : lecture 315 / 319 / 312 sur 360 ;
      écriture stricte 93 % / 88 % / 89 % ; vide qui parle 215 / 212 / 214
      sur 240 (JSON naïf : 68).
    - Pour les modèles forts, l'arbre indenté est à égalité avec le JSON, à
      une question près. Les modèles faibles font des erreurs partout, et un
      peu plus sur l'arbre indenté en lecture et en écriture.
    - Le JSON naïf échoue sur le vide chez tous les modèles.
    - Coût OpenRouter : 5,37 $.
  - **Données réelles et polymorphisme** (2026-11-24, `scripts/real_data_benchmark.py`).
    Les jeux : 12 réponses d'API publiques (PyPI, npm, crates.io, Wikidata).
    - Tokens comptés par 6 fournisseurs : −39 à −49 % sur les données
      tabulaires imbriquées, mais seulement −8 à −13 % sur les données
      réelles riches en objets.
    - Lecture : parité, mais les questions générées sont trop faciles
      (effet plafond).
    - Écriture par un modèle sur données réelles : nettement moins fiable
      que JSON. GPT-5 mini 5/12 contre 12/12, Llama 3/12 contre 10/12,
      Claude 10/12 contre 12/12.
    - Permettre le JSON pur dans les valeurs en ligne n'aide pas (GPT-5 mini
      6/12, Llama 3/12). Les modèles inventent de la syntaxe à partir d'un
      seul exemple : `^@@`, colonnes séparées par des virgules, `nom:valeur`.
    - Conclusion : sur des données irrégulières, l'écriture par un modèle
      reste le point faible.
  - **Forme « strict »** (ancien nom « objets_json », renommée le
    2026-11-25) pour les données irrégulières. C'est la forme d'écriture des
    modèles **jusqu'à ce qu'ils soient entraînés** sur les formes CSTL plus
    compactes. Elle ressemble à un sous-ensemble du YAML, sans ses pièges ;
    `#cstl objets_json` reste lu. Mesures (2026-11-24) :
    clés sans guillemets, indentation, ni accolades ni virgules, mais chaque
    valeur écrite comme en JSON.
    - Écriture des 12 réponses d'API réelles : **48/48** sur GPT-5 mini,
      Claude Haiku, Llama 4 et Mistral, contre 42/48 en JSON, 25/48 en arbre
      indenté et 29/48 en forme « objets » à valeurs CSTL.
    - Coût : +7 à +18 % de tokens face au JSON compact. On paie la fiabilité.
    - Une seule passe : à confirmer.
  - **Formes mixte et changements** (2026-11-24, `sdk/python/cstl_formes.py`).
    - **mixte** : la structure d'`objets_json`, avec les listes d'objets en
      tables à cellules JSON. Sur les données réelles, −3 à −8 % de tokens
      face au JSON. Écriture : 37/48, contre 42 pour le JSON et 48 pour
      `objets_json`. Les échecs viennent des tables dont certains éléments
      n'ont pas tous les champs : le modèle décale les valeurs. Elle sert
      donc à transmettre, pas à faire écrire un modèle.
    - **changements** : opérations JSON Patch d'une ligne, avec `diff` et
      `apply`.
  - **Forme « relationnel »** (2026-11-24, `cstl_formes.py`) : un format
    CSTL propre aux données irrégulières, pour ne pas copier le JSON.
    - Principe : valeurs JSON ; un objet à un seul champ s'écrit en chemin
      (`engines>node: ">=18"`) ; une table seulement si tous les éléments ont
      exactement les mêmes champs ; `[N]` ; `?` pour l'inconnu.
    - Coût : −6 à 0 % de tokens face au JSON compact.
    - Écriture des 12 réponses d'API réelles (GPT-5 mini, Claude Haiku,
      Llama 4, Mistral) :
      - v1 (chemins `a.b`) : 40/48. Deux erreurs de conception : les vraies
        clés contiennent des points (`2.34.0`) ; les modèles écrivent
        plusieurs chemins vers le même objet.
      - v2 (séparateur `>`, chemins fusionnés) : 36/48 mesurée. Trois
        modèles échouaient sur la même clé à espace (`Release notes`),
        écrite sans guillemets comme la consigne le dit. Le lecteur accepte
        maintenant une clé jusqu'au premier `: ` (comme en YAML) : 40/48 en
        relisant les mêmes réponses (pas une nouvelle mesure).
    - Échecs restants : du contenu, pas de la syntaxe (un `id` inventé, une
      liste vide ajoutée, des éléments omis par Llama).
    - Conclusion : plus compact que `objets_json`, moins fiable (40 contre
      48), à peu près égal au JSON (42). `choose_format(..., "ecrire")`
      garde `objets_json` pour faire écrire un modèle ; entre machines, la
      forme la moins chère l'emporte.
  - **Plusieurs exemples et tour de correction** (2026-11-24,
    `real_data_benchmark.py --examples N --repair`, une passe, 4 modèles,
    12 documents réels). Le tour de correction renvoie seulement l'erreur du
    vrai lecteur.

    | Écriture | JSON | objets_json | relationnel |
    |---|---|---|---|
    | 1 exemple, du premier coup | 36/48 | 48/48 | 32/48 |
    | 1 exemple + correction | 36/48 | 48/48 | 40/48 |
    | 3 exemples, du premier coup | 47/48 | 46/48 | 44/48 |
    | 3 exemples + correction | 47/48 | 46/48 | 44/48 |

    - Avec 1 exemple, les 12 échecs du JSON sont **tous silencieux** (JSON
      valide, contenu faux). Les 16 de `relationnel` sont **tous
      détectés** par le lecteur, mais la correction n'en répare que 8.
    - Trois exemples aident surtout le JSON (36 → 47). Les écarts de 1 à
      2 points sont du bruit : le JSON avait fait 42/48 à 1 exemple lors
      d'une passe précédente.
    - Avec de bons exemples, aucune forme CSTL ne bat le JSON en écriture
      sur ces données. `objets_json` reste le seul à 48/48 avec 1 exemple,
      la consigne la plus courte. L'avantage mesuré de CSTL ici, c'est la
      détection des erreurs, pas le score.
  - **Cinq formes spécialisées** : `serie`, `graphe`, `conversation`,
    `appels` et `matrice`. Sans perte, mais pas encore mesurées sur des
    modèles.
  - **Fiabilité des formes de relations** (4 modèles, 48 politiques) :
    - lecture : lignes 26, symboles 25, complet 22, tableau 18 ;
    - écriture : complet 24, symboles 21, lignes 17, tableau 15.
  - **CSTL polymorphe** (2026-11-24) : `cstl_compact.encode_auto(data)`
    choisit la forme de CSTL selon les données.
    - Relations : `complet`, `lignes`, `tableau` ou `symboles`.
    - Documents : `arbre`, `arbre_n` (avec `/N`, imposé si un modèle lit)
      ou `arbre_ids`.
    - Chaque forme candidate est vérifiée sans perte, ordre des règles
      compris, et la moins chère en tokens estimés l'emporte.
    - Le texte commence par `#cstl <forme>` ; `decode_auto` le relit.
    - Choix observés : politiques de 30 à 60 règles → `symboles` (2,5 fois
      moins cher que `complet`) ; 10 règles → `lignes` ; documents → `arbre`.
    - Depuis le 2026-11-24, `choose_format(data, task)` choisit **la forme la
      plus fiable mesurée** quand un modèle lit (`task="lire"`) ou écrit
      (`task="ecrire"`), selon la sorte de données (relations, tabulaire,
      irrégulier ; table `RELIABILITY`). La moins chère ne départage qu'à
      fiabilité égale. Entre machines (`task=None`), la moins chère.
    - Choix : politique → lignes pour lire, complet pour écrire ; données
      tabulaires → arbre_n ; données irrégulières à écrire → objets_json.
    - Vérifié sur 3 000 documents et 2 000 listes de relations aléatoires
      sans perte.
  - Le mode arbre passe par le serveur : `agent.send_tree` envoie l'arbre
    en relations signées, vérifiées (`SCHEMA`) et auditées, et
    `agent.fetch_tree` le relit à l'identique (`purpose=tree_query`).
  - Limite : tout agent enregistré peut relire un arbre s'il connaît son
    id.

- **Compréhension des 18 domaines sur Gemma 4 31B (2026-11-22, partiel).**
  Le modèle ouvert de Google a été testé avec les questions qui mélangent
  « permis » et « interdit ».
  - 47 cas notés, tous parfaits : JSON 72/72, TOON 72/72, CSTL compact
    78/78 et CSTL sans mot-clé 60/60.
  - Une coupure réseau a interrompu le test, et 3 cas ont échoué côté API.
  - Résultats : `scripts/domains_comprehension_gemma-4-31b-it_partiel.json`.

## Déjà fait (PR #3 à #11)

- **Couche 9, autorisation avant action :** jetons signés, garde côté
  système visé, auteurs de politique, révision des règles, politique
  effective.
- **Conditions vérifiées par le serveur :** rôles, période de validité,
  montants, faits.
- **Chaîne d'audit :** signée par le serveur, suppression des derniers
  maillons détectée, témoin externe contre le retour en arrière, rotation
  de la clé du serveur.
- **Sécurité par défaut :** signatures obligatoires, et identité conservée
  au redémarrage (faille corrigée).

## Délibération v6 (2026-11-26)

- **Fait :** le moteur de délibération (`sdk/python/cstl_deliberation.py`,
  spécification `docs/CSTL_DELIBERATION_v6.md`) :
  - points numérotés, état au lieu de l'historique ;
  - vérification locale, puis par le serveur, avec correction par le
    modèle ;
  - CONSENSUS et DEADLOCK, avec des garde-fous contre les boucles ;
  - compilation en règles signées, escalade humaine ;
  - mode manuel et OpenRouter ; testé avec des modèles déterministes.
- **Mesuré (2026-11-26, `benchmark/deliberation/README.md`)** : 26 cas, dont
  6 adverses, avec 3 modèles gratuits. Aucune erreur, que ce soit pour l'agent
  seul, le débat libre ou CSTL v6. CSTL v6 coûte 4 fois moins de tokens que
  le débat libre, mais 5 à 7 fois plus que l'agent seul. Délibérer n'a donc
  pas amélioré la justesse ici.
- **À faire :**
  - **Essai sur petits modèles arrêté** (2026-11-26) : les petits modèles
    gratuits se trompent bien seuls, mais deux sur trois ne répondent plus
    sous CSTL v6, donc rien n'est concluant ;
  - **corrigé** (2026-11-26) : un CONSENSUS exige un quorum (majorité, au
    moins 2), et ne vient donc plus d'un seul participant restant ;
  - **corrigé** (2026-11-26) : le serveur refuse une ligne de prose hors
    bloc (`unrecognized_line`) et un `sigma` hors de 0 à 1
    (`invalid_sigma`) ;
  - départager sur des tâches où l'agent seul se trompe : petits modèles,
    dossiers très longs, règles implicites ;
  - les preuves externes pour `sigma` ;
  - **fait** (2026-11-27) : une vérification indépendante de l'état publié par
    le modérateur (`verify_state`) ;

## Premier essai réel dans claude.ai (2026-11-27)

- **Le serveur CSTL tourne sur Render.** Il est construit depuis
  `render.yaml` en 4 min 27 s, sur l'offre gratuite. Le connecteur est
  ajouté dans claude.ai depuis un téléphone.
- **Claude, dans claude.ai, demande l'autorisation, et le serveur
  décide :**
  - `delete_repo` refusé ;
  - `send_payment` de 30 autorisé, avec un jeton signé ;
  - `send_payment` de 200 refusé.
- **Vrai contrôle.** Appelés **directement**, sans demande, les outils
  gardés (`demo_delete_repo`, `demo_send_payment` de 200) ont refusé
  d'eux-mêmes. L'état final le confirme : `prod-db` existe toujours, et un
  seul paiement est passé.
- **Corrigé le même jour :**
  - le 401 sur les adresses inconnues faisait croire à claude.ai que le
    serveur utilisait OAuth : c'est maintenant un 404 ;
  - le serveur CSTL restait en vie quand `cstl cloud` était arrêté
    (SIGTERM) : l'arrêt est maintenant propagé.
- **Ajouté ensuite :**
  - de vraies actions GitHub gardées (issues). **Essayé contre le vrai
    GitHub le même jour,** sur un dépôt d'essai, avec un jeton limité aux
    issues :
    - création d'une issue et commentaire, permis : exécutés ;
    - fermeture, interdite et appelée directement : refusée, et GitHub n'a
      pas été appelé ; l'issue est restée ouverte ;
  - `--compact-results` dans le proxy MCP. Le point « convertisseur » de la
    section suivante est donc **fait** pour le proxy ; la mesure en tokens
    réels reste à faire.

## Revue d'attaque (2026-11-27, `docs/RED_TEAM_2026-11-27.md`)

- **En direct et sur une copie locale, aucune attaque n'a permis une action
  interdite.** Attaques tentées :
  - règles et rôle que l'IA s'attribue elle-même ;
  - pièges sur les montants ;
  - jeton, origine et sessions ;
  - requêtes malformées et injections.
- **Corrigé :**
  - limite de débit et de sessions du serveur MCP distant ;
  - outils d'écriture de relations coupés par défaut ;
  - refus clair, avant signature, d'une valeur que le fil ne porte pas ;
  - démonstration coupée dans `render.yaml`.
- **À faire pour un vrai usage :**
  - audit persistant (disque et témoin) ;
  - clé de l'auteur hors du serveur.

## À faire : convertisseur de tokens et offre commerciale (2026-11-27)

- **Convertisseur dans le proxy MCP.** Transformer automatiquement le JSON
  des résultats d'outils en arbre indenté avant qu'il arrive au modèle,
  sans perte (vérifié sur 30 000 documents), sans rien changer au serveur
  MCP ni au client.
  - Mesurer le gain réel sur de vrais résultats d'outils MCP : fichiers,
    GitHub, bases de données, API. On a mesuré −39 % sur les données
    imbriquées et −8 à −13 % sur les données riches en objets.
  - Mesurer aussi que le modèle comprend aussi bien les résultats
    convertis.
  - Ne convertir que lorsque c'est plus court, sinon laisser le JSON.
- **Offre commerciale, à valider avant de construire plus.**
  - Le code est sous Apache 2.0 : ce qui se vend, c'est le service. Une
    passerelle hébergée réunirait le contrôle des actions et la conversion
    des tokens, contre un abonnement ou un pourcentage des tokens
    économisés.
  - Modèle « open core » : la base reste libre, et les fonctions pour
    entreprises sont payantes (tableau de bord avancé, PostgreSQL,
    authentification d'entreprise, support).
  - D'abord trouver 3 personnes prêtes à payer, puis mesurer leurs propres
    données.
  - Conseil juridique et fiscal à prendre auprès d'un professionnel.

## Publication (2026-11-26)

- **Le nom `cstl` est déjà pris sur PyPI** par un portage Python de la
  bibliothèque standard C++ : le paquet Python ne peut pas être publié sous
  ce nom. Il est libre sur npm. Libres sur les deux : `cstl-control` et
  `cstl-agents`. C'est à l'auteur de choisir.
- **Prêt pour les pilotes :**
  - le guide `docs/PILOT.md` ;
  - le brouillon d'article `docs/ARTICLE_DRAFT.md` ;
  - le Dockerfile (serveur et 8 commandes, données dans `/data`) ; image construite et vérifiée le 2026-11-27
    (246 Mo, `cstl-demo` 7/7 dans le conteneur).
- **Banc contre de vrais outils (OPA avec paquets signés, Cedar) : pas fait
  ici.** Télécharger et lancer leurs binaires a été refusé dans
  l'environnement de développement. À faire sur une machine où on peut les
  installer.
- **Licence : Apache 2.0** (choisie le 2026-11-27, fichier `LICENSE`).

