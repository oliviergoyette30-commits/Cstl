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
  - La compréhension reste à mesurer.
  - Le mode arbre passe par le serveur : `agent.send_tree` envoie l'arbre
    en relations signées, vérifiées (`SCHEMA`) et auditées, et
    `agent.fetch_tree` le relit à l'identique (`purpose=tree_query`).
  - Limite : tout agent enregistré peut relire un arbre s'il connaît son
    id.

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
