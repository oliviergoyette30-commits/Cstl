# Test tripartite du 2026-11-26 : notes

Sujet : un hôpital doit-il laisser un agent IA commander seul des analyses
de laboratoire ? Conversation recopiée à la main par l'utilisateur, puis
vérifiée par `tripartite.py check --start gemini`. Le rapport est dans
`rapport.md`.

**Qui a écrit quoi**
- **ChatGPT et Gemini :** leurs sites web (chatgpt.com, gemini.google.com),
  réponses collées telles quelles.
- **Claude :** les 4 messages de Claude ont été écrits par la session
  Claude Code qui gère ce dépôt, et pas par claude.ai. Ils ont été vérifiés
  par le serveur avant d'être envoyés. Le score de 100 % de Claude n'est
  donc pas comparable à celui des deux autres : Claude pouvait se vérifier
  lui-même avant d'envoyer.

**Ce qui manque**
- Le tour 1 (ouverture) et la réponse de ChatGPT au tour 2 ne sont pas dans
  le transcript.
- Le transcript commence à la réponse de Gemini au tour 2, dans sa version
  refusée, puis dans sa version corrigée après le message d'erreur du
  serveur.

**Constats**
- **Opérateurs inventés.** Les deux modèles ont mis en majuscules des mots
  qui ne sont pas des opérateurs : `AGREES`, `PARTIAL_DISPUTE` (un type de
  bloc pris pour un opérateur), `DECIDE`. Le serveur les refuse (E101).
  Avec le message d'erreur exact, Gemini s'est corrigé en un seul essai.
- **Une ligne `---` en trop** avant `---END---` : 1 fois chez Gemini, 2 fois
  chez ChatGPT. Le serveur l'ignore en silence ; le vérificateur la
  signale.
- **Confiance.** Gemini et ChatGPT mettent 0,9 à 0,99 sur presque tout, y
  compris pour affirmer que « la confiance doit refléter l'incertitude ».
  Le serveur avertit à chaque fois que Gemini met `ASSUMES` trop haut
  (W613, 3 fois).
- **Décision.** ChatGPT a écrit sa `DECISION:` en longue phrase française,
  au lieu d'un identifiant. Gemini n'a pas écrit de ligne `DECISION:` du
  tout, et a recopié les consignes du modérateur comme ses propres
  relations.
- **Fond.** Les trois ont convergé sur l'essentiel : des catalogues par
  service classés par risque, l'escalade immédiate des valeurs critiques,
  une version figée et revalidée, un responsable nommé, un pilote. Il reste
  un désaccord final :
  - ChatGPT et Claude veulent une autonomie limitée (`MAY`) pour les
    analyses de routine du catalogue ;
  - Gemini veut la signature d'un médecin sur chaque commande.
- **Limite du vérificateur.** Ce désaccord n'apparaît pas dans la section
  « désaccords déontiques », parce que chaque modèle nomme l'action à sa
  façon. Les identifiants libres empêchent de comparer automatiquement les
  positions. Il faudrait un vocabulaire d'actions commun, donné dans
  l'amorce.
