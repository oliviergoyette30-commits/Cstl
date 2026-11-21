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
