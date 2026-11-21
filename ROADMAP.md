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
