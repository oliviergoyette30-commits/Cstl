//! src/server/action_authorization.rs -- Couche 9 : autorisation AVANT action
//! (2026-11-20).
//!
//! Probleme : CSTL ne se trouve pas sur le chemin des actions des agents (ils
//! agissent dans leurs propres processus). Les regles deontiques ne pouvaient
//! donc que JUGER APRES COUP un message deja envoye. Ici, l'agent demande
//! d'abord : `INTENT_PAYLOAD [purpose=action_request, action=<nom>]`, signe.
//! Le serveur repond `allowed` ou `denied` d'apres l'historique persiste des
//! obligations / interdictions, et inscrit la decision dans la chaine d'audit.
//!
//! Regles de decision (pures, sans E/S, testees ci-dessous) :
//! 1. identite : demande non signee, ou expediteur non enregistre avec une
//!    cle -> `denied` (autoriser une identite non prouvee n'a pas de sens) ;
//! 2. `action` absente ou hors de `[A-Za-z0-9_.-]{1,128}` -> `denied`
//!    (la valeur est recopiee dans la reponse wire : jamais de `,` ni de `]`) ;
//! 3. une INTERDICTION (MUST_NOT / FORBID) dans l'historique pour
//!    (sujet = expediteur, objet = action) -> `denied`, meme si une
//!    obligation contraire existe aussi : l'interdiction l'emporte, et le
//!    conflit est signale ;
//! 4. sinon, une OBLIGATION (MUST / REQUIRE) -> `allowed` (required) ;
//! 5. sinon -> politique par defaut : `allowed` (`CSTL_ACTION_DEFAULT` absent
//!    ou `allow`), ou `denied` (`CSTL_ACTION_DEFAULT=deny` : liste blanche,
//!    seule une obligation explicite autorise).
//!
//! AUTORITE ET REVISION (2026-11-20, apres le banc d'essai d'equipe
//! `scripts/team_task_benchmark.py`, ou la couche 9 a refuse 3 actions
//! correctes) : avec `CSTL_POLICY_AUTHORS=nom1,nom2,prefixe*`,
//! - seules comptent les regles affirmees par un auteur de cette liste, dans
//!   un message dont la signature est valide ET faite avec la cle enregistree
//!   pour cet auteur (`Rule::authorized`, calcule par l'appelant) ; les autres
//!   sont ignorees et comptees (`ignored`) ;
//! - la regle autorisee la PLUS RECENTE decide (une interdiction peut donc
//!   etre levee par l'autorite, et une obligation retiree) ; `superseded`
//!   signale qu'une regle contraire plus ancienne a ete remplacee.
//!
//! Sans `CSTL_POLICY_AUTHORS`, comportement d'origine : toute regle compte et
//! l'interdiction l'emporte toujours. La revision n'est PAS activee dans ce
//! mode : si n'importe qui pouvait reviser, n'importe qui pourrait lever une
//! interdiction en envoyant simplement un MUST plus recent.

use crate::semantic::{FORBIDDEN_MODALITIES, REQUIRED_MODALITIES};

/// Permission sans obligation (2026-11-21). Sans elle, « autorise jusqu'a
/// 200 euros » ne pouvait s'ecrire qu'avec MUST, qui en fait une OBLIGATION.
/// Ne contredit ni MUST ni MUST_NOT dans l'audit deontique (il n'y figure pas).
pub const PERMITTED_MODALITIES: &[&str] = &["MAY", "PERMIT"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultPolicy {
    Allow,
    Deny,
}

impl DefaultPolicy {
    pub fn from_env() -> Self {
        match std::env::var("CSTL_ACTION_DEFAULT") {
            Ok(v) if v.eq_ignore_ascii_case("deny") => DefaultPolicy::Deny,
            _ => DefaultPolicy::Allow,
        }
    }
}

/// Qui peut fixer les regles qui comptent pour la couche 9.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyAuthors {
    /// Toute regle compte ; l'interdiction l'emporte (comportement d'origine).
    Anyone,
    /// Seules comptent les regles prouvees de ces auteurs (`nom` exact ou
    /// `prefixe*`) ; la plus recente decide.
    Only(Vec<String>),
}

impl PolicyAuthors {
    pub fn from_env() -> Self {
        Self::parse(std::env::var("CSTL_POLICY_AUTHORS").ok().as_deref())
    }

    pub fn parse(value: Option<&str>) -> Self {
        let names: Vec<String> = value
            .unwrap_or("")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if names.is_empty() { PolicyAuthors::Anyone } else { PolicyAuthors::Only(names) }
    }

    /// Le nom fait-il partie des auteurs de politique ? (`Anyone` : oui.)
    pub fn includes(&self, name: &str) -> bool {
        match self {
            PolicyAuthors::Anyone => true,
            PolicyAuthors::Only(list) => list.iter().any(|p| match p.strip_suffix('*') {
                Some(prefix) => name.starts_with(prefix),
                None => p == name,
            }),
        }
    }
}

/// Actions GOUVERNEES (2026-11-25, `CSTL_GOVERNED_ACTIONS`, noms exacts ou
/// `prefixe*`, separes par des virgules). Une action gouvernee n'est permise
/// que si un auteur de politique a ecrit AU MOINS UNE regle pour elle (pour
/// cet agent), qu'elle s'applique ou non ; s'il en existe, la decision reste
/// celle de `decide`. Sans regle : refus `governed_no_policy`.
///
/// Pourquoi (scripts/competition_benchmark.py, Llama 4 et Mistral) : quand
/// la politique de l'auteur n'arrive pas (rien d'ecrit, ou message refuse
/// pour contradiction), un serveur ouvert par defaut laissait passer un
/// remboursement de 2500. Le mode ferme global corrigeait ca, mais refusait
/// 5 actions legitimes jamais mentionnees (benchmark/). La liste est fixee a
/// l'INSTALLATION, pas dans la politique : un auteur qui n'ecrit rien ne
/// peut pas la vider. Une regle d'un non-auteur (initie) ne compte pas.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GovernedActions(Vec<String>);

impl GovernedActions {
    pub fn from_env() -> Self {
        Self::parse(std::env::var("CSTL_GOVERNED_ACTIONS").ok().as_deref())
    }

    pub fn parse(value: Option<&str>) -> Self {
        GovernedActions(value.unwrap_or("").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
    }

    pub fn includes(&self, action: &str) -> bool {
        self.0.iter().any(|p| match p.strip_suffix('*') {
            Some(prefix) => action.starts_with(prefix),
            None => p == action,
        })
    }

    pub fn patterns(&self) -> &[String] {
        &self.0
    }
}

/// Applique la regle des actions gouvernees a une decision de `decide`.
/// Aucune regle comptee pour l'action = ni regle qui a tranche, ni regle
/// dont les conditions ne s'appliquent pas.
pub fn govern(d: Decision, action: Option<&str>, governed: &GovernedActions) -> Decision {
    match action {
        Some(a) if d.allowed && d.rule.is_none() && d.not_applicable == 0 && governed.includes(a) => {
            Decision { allowed: false, reason: "governed_no_policy", ..d }
        }
        _ => d,
    }
}

/// Conditions d'une regle (2026-11-21), lues dans les attributs de sa
/// RELATION et VERIFIEES par le serveur (avant, seule la raison suivait la
/// regle : "interdit de rembourser sans accord" ne pouvait etre applique que
/// comme "interdit de rembourser, point").
///   valid_from / valid_until : periode ou la regle est en vigueur (date ISO
///       8601, ex. 2026-12-31 ou 2026-12-31T23:59:59Z, ou secondes unix) ;
///   amount_above=X   : la regle ne vaut que si le montant demande > X ;
///   amount_at_most=X : la regle ne vaut que si le montant demande <= X ;
///   if_fact=F        : la regle ne vaut que si le fait F est vrai ;
///   unless_fact=F    : la regle ne vaut que si le fait F est faux.
/// Un fait a TROIS etats (2026-11-22) : vrai (`RELATION [type=STATE,
/// subject=F, object=true]`), faux (`object=false`) ou INCONNU (jamais
/// declare). Le plus recent compte ; en mode `Only`, seuls ceux d'un auteur de
/// politique comptent.
///
/// Principe de prudence : une condition qu'on ne peut pas evaluer (montant
/// absent de la demande, attribut illisible, fait inconnu) fait APPLIQUER une
/// interdiction et NE PAS appliquer une obligation ni une permission.
/// Avant le 2026-11-22, un fait inconnu valait faux : « interdit de liquider
/// s'il y a un incident » (`if_fact=incident_open`) etait LEVE tant que
/// personne n'avait declare l'etat de l'incident.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Conditions {
    pub valid_from: Option<u64>,
    pub valid_until: Option<u64>,
    pub amount_above: Option<f64>,
    pub amount_at_most: Option<f64>,
    /// Total CUMULE (2026-11-21) : montants deja autorises a cet agent pour
    /// cette action, plus celui demande. `total_above=X` : la regle ne vaut
    /// que si ce total depasse X ; `total_at_most=X` : seulement s'il reste
    /// au plus X. `total_window_secs` : fenetre glissante (sinon, depuis
    /// toujours). Additionner des dizaines de montants est une faiblesse
    /// connue des LLM : ici, c'est le serveur qui compte.
    pub total_above: Option<f64>,
    pub total_at_most: Option<f64>,
    pub total_window_secs: Option<u64>,
    pub if_fact: Option<String>,
    pub unless_fact: Option<String>,
    /// Priorite explicite (2026-11-22), entier de -1000 a 1000, 0 par defaut.
    /// En mode `Only` (auteurs de politique), les regles sont classees d'abord
    /// par priorite, puis par date : une exception (« MAY ... priority=10 »)
    /// l'emporte sur une interdiction generale de priorite 0, meme plus
    /// recente. En mode `Anyone`, elle est IGNOREE : n'importe quel agent
    /// pourrait sinon lever une interdiction avec une priorite elevee.
    pub priority: Option<i64>,
    /// Attributs de condition illisibles (nom=valeur).
    pub invalid: Vec<String>,
}

/// Date ISO 8601 (YYYY-MM-DD, ou avec heure et fuseau) ou secondes unix.
pub fn parse_time(v: &str) -> Option<u64> {
    let v = v.trim();
    if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) {
        return v.parse().ok();
    }
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(v) {
        return u64::try_from(t.timestamp()).ok();
    }
    if let Ok(t) = chrono::NaiveDateTime::parse_from_str(v, "%Y-%m-%dT%H:%M:%S") {
        return u64::try_from(t.and_utc().timestamp()).ok();
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d") {
        return u64::try_from(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp()).ok();
    }
    None
}

/// Montant : nombre fini, positif ou nul.
pub fn parse_amount(v: &str) -> Option<f64> {
    v.trim().parse::<f64>().ok().filter(|a| a.is_finite() && *a >= 0.0)
}

impl Conditions {
    pub const KEYS: [&'static str; 10] = ["valid_from", "valid_until", "amount_above", "amount_at_most",
        "total_above", "total_at_most", "total_window_secs", "if_fact", "unless_fact", "priority"];

    pub fn from_attrs(attrs: &std::collections::HashMap<String, String>) -> Self {
        let mut c = Conditions::default();
        for (k, v) in attrs {
            let bad = |c: &mut Conditions| c.invalid.push(format!("{k}={v}"));
            match k.as_str() {
                "valid_from" => match parse_time(v) { Some(t) => c.valid_from = Some(t), None => bad(&mut c) },
                "valid_until" => match parse_time(v) { Some(t) => c.valid_until = Some(t), None => bad(&mut c) },
                "amount_above" => match parse_amount(v) { Some(a) => c.amount_above = Some(a), None => bad(&mut c) },
                "amount_at_most" => match parse_amount(v) { Some(a) => c.amount_at_most = Some(a), None => bad(&mut c) },
                "total_above" => match parse_amount(v) { Some(a) => c.total_above = Some(a), None => bad(&mut c) },
                "total_at_most" => match parse_amount(v) { Some(a) => c.total_at_most = Some(a), None => bad(&mut c) },
                "total_window_secs" => match v.trim().parse::<u64>() { Ok(t) if t > 0 => c.total_window_secs = Some(t), _ => bad(&mut c) },
                "if_fact" if is_valid_action(v) => c.if_fact = Some(v.clone()),
                "unless_fact" if is_valid_action(v) => c.unless_fact = Some(v.clone()),
                "if_fact" | "unless_fact" => bad(&mut c),
                "priority" => match v.trim().parse::<i64>() { Ok(p) if (-1000..=1000).contains(&p) => c.priority = Some(p), _ => bad(&mut c) },
                _ => {}
            }
        }
        c.invalid.sort();
        c
    }

    fn has_amount(&self) -> bool {
        self.amount_above.is_some() || self.amount_at_most.is_some() || self.has_total()
    }

    fn has_total(&self) -> bool {
        self.total_above.is_some() || self.total_at_most.is_some()
    }

    /// Paires (nom, valeur) a montrer a l'agent (wire : valeurs sans `,`/`]`).
    pub fn describe(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        let date = |t: u64| {
            chrono::DateTime::from_timestamp(t as i64, 0).map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_else(|| t.to_string())
        };
        if let Some(t) = self.valid_from { out.push(("valid_from", date(t))); }
        if let Some(t) = self.valid_until { out.push(("valid_until", date(t))); }
        if let Some(a) = self.amount_above { out.push(("amount_above", a.to_string())); }
        if let Some(a) = self.amount_at_most { out.push(("amount_at_most", a.to_string())); }
        if let Some(a) = self.total_above { out.push(("total_above", a.to_string())); }
        if let Some(a) = self.total_at_most { out.push(("total_at_most", a.to_string())); }
        if let Some(w) = self.total_window_secs { out.push(("total_window_secs", w.to_string())); }
        if let Some(p) = self.priority { out.push(("priority", p.to_string())); }
        if let Some(f) = &self.if_fact { out.push(("if_fact", f.clone())); }
        if let Some(f) = &self.unless_fact { out.push(("unless_fact", f.clone())); }
        if !self.invalid.is_empty() { out.push(("invalid_conditions", self.invalid.len().to_string())); }
        out
    }
}

/// Ce que le serveur sait de la demande en cours, pour evaluer les conditions.
#[derive(Debug, Clone, Default)]
pub struct RequestContext {
    /// Secondes unix.
    pub now: u64,
    /// Montant demande (`INTENT_PAYLOAD [..., amount=...]`), s'il y en a un.
    pub amount: Option<f64>,
    /// Faits declares VRAIS (voir `Conditions`).
    pub facts: std::collections::HashSet<String>,
    /// Faits declares FAUX. Un fait absent des deux ensembles est inconnu.
    pub false_facts: std::collections::HashSet<String>,
    /// Roles de l'expediteur (`RELATION [type=POSSESSES, subject=<agent>,
    /// object=role:<nom>]`, pose par un auteur de politique en mode `Only`).
    pub roles: std::collections::HashSet<String>,
    /// Montants deja AUTORISES a l'expediteur pour l'action demandee :
    /// (secondes unix, montant). Pour `total_above` / `total_at_most`.
    pub spent: Vec<(u64, f64)>,
}

impl RequestContext {
    /// Somme des montants deja autorises, dans la fenetre si elle est donnee.
    pub fn spent_within(&self, window: Option<u64>) -> f64 {
        self.spent
            .iter()
            .filter(|(t, _)| window.is_none_or(|w| t + w >= self.now))
            .map(|(_, a)| a)
            .sum()
    }

    /// Etat d'un fait : `Some(true)` vrai, `Some(false)` faux, `None` inconnu.
    pub fn fact(&self, name: &str) -> Option<bool> {
        if self.facts.contains(name) {
            Some(true)
        } else if self.false_facts.contains(name) {
            Some(false)
        } else {
            None
        }
    }

    /// Enregistre un fait (le plus recent l'emporte).
    pub fn set_fact(&mut self, name: &str, value: bool) {
        if value {
            self.false_facts.remove(name);
            self.facts.insert(name.to_string());
        } else {
            self.facts.remove(name);
            self.false_facts.insert(name.to_string());
        }
    }

    pub fn now() -> Self {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        RequestContext { now, ..Default::default() }
    }
}

/// Une regle deontique persistee, dans l'ordre ou elle a ete affirmee
/// (la plus ancienne d'abord).
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// Un agent, ou `role:<nom>` : tous les agents qui ont ce role.
    pub subject: String,
    pub object: String,
    pub modality: String,
    /// Auteur dans `PolicyAuthors` ET identite prouvee (signature valide,
    /// cle = cle enregistree de l'auteur). Ignore en mode `Anyone`.
    pub authorized: bool,
    /// Expediteur du message qui a pose la regle.
    pub author: Option<String>,
    /// Raison donnee avec la regle (`RELATION [..., reason=...]`), conditions
    /// comprises : sans elle, un agent ne voit qu'un `MUST_NOT` nu (constate
    /// le 2026-11-21 : Opus choisissait alors plus souvent l'action interdite).
    pub reason: Option<String>,
    /// Si `authorized` est faux en mode `Only` : pourquoi.
    pub why_ignored: Option<&'static str>,
    pub conditions: Conditions,
}

impl Rule {
    fn forbids(&self) -> bool {
        FORBIDDEN_MODALITIES.contains(&self.modality.as_str())
    }

    /// La regle vise-t-elle cet agent (par son nom ou l'un de ses roles) ?
    fn targets(&self, sender: &str, ctx: &RequestContext) -> bool {
        self.subject == sender || self.subject.strip_prefix("role:").is_some_and(|r| ctx.roles.contains(r))
    }

    /// En vigueur maintenant, et ses conditions de faits remplies.
    fn in_force(&self, ctx: &RequestContext) -> bool {
        let c = &self.conditions;
        if !c.invalid.is_empty() {
            return self.forbids();
        }
        if c.valid_from.is_some_and(|t| ctx.now < t) || c.valid_until.is_some_and(|t| ctx.now > t) {
            return false;
        }
        for (fact, wanted) in [(&c.if_fact, true), (&c.unless_fact, false)] {
            let Some(f) = fact else { continue };
            match ctx.fact(f) {
                Some(v) if v == wanted => {}
                Some(_) => return false,
                // Fait inconnu : prudence, comme pour un montant absent.
                None => {
                    if !self.forbids() {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// S'applique a CETTE demande (montant compris).
    fn applies(&self, ctx: &RequestContext) -> bool {
        if !self.in_force(ctx) {
            return false;
        }
        let c = &self.conditions;
        if !c.has_amount() {
            return true;
        }
        match ctx.amount {
            None => self.forbids(),
            Some(a) => {
                let total = a + ctx.spent_within(c.total_window_secs);
                !c.amount_above.is_some_and(|x| a <= x)
                    && !c.amount_at_most.is_some_and(|x| a > x)
                    && !c.total_above.is_some_and(|x| total <= x)
                    && !c.total_at_most.is_some_and(|x| total > x)
            }
        }
    }
}

/// Une regle telle qu'elle s'applique a une action.
#[derive(Debug, Clone, PartialEq)]
pub struct PolicyEntry {
    pub action: String,
    pub modality: String,
    pub author: Option<String>,
    pub reason: Option<String>,
    /// Pour une regle ignoree : pourquoi.
    pub why_ignored: Option<&'static str>,
    /// Regle visant un role (`role:<nom>`) plutot que l'agent lui-meme.
    pub via_role: Option<String>,
    pub conditions: Conditions,
    /// Regle sans condition suivie de regles a montant : elle ne decide que
    /// « dans les autres cas » (montant non couvert, ou absent).
    pub otherwise: bool,
}

/// Politique EFFECTIVE d'un agent (2026-11-21, `purpose=policy_query`) :
/// ce que la couche 9 decidera, action par action, avec les raisons et les
/// conditions -- et les regles qu'elle ignore. Avant, l'agent ne voyait que
/// les messages bruts des autres agents, y compris ceux d'agents sans
/// autorite (cas 11 du banc d'equipe : Haiku obeissait a une interdiction
/// que le serveur ignorait), et rien ne lui rappelait ses obligations.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EffectivePolicy {
    /// Obligations : a faire, meme si la demande de travail ne les cite pas.
    pub required: Vec<PolicyEntry>,
    /// Permissions (`MAY`) : autorise, pas obligatoire.
    pub permitted: Vec<PolicyEntry>,
    pub forbidden: Vec<PolicyEntry>,
    pub ignored: Vec<PolicyEntry>,
}

fn entry(r: &Rule) -> PolicyEntry {
    PolicyEntry {
        action: r.object.clone(),
        modality: r.modality.clone(),
        author: r.author.clone(),
        reason: r.reason.clone(),
        why_ignored: r.why_ignored,
        via_role: r.subject.strip_prefix("role:").map(str::to_string),
        conditions: r.conditions.clone(),
        otherwise: false,
    }
}

/// Meme logique que `decide`, pour toutes les actions qui ont une regle en
/// vigueur. Les regles a condition de MONTANT ne peuvent pas etre resolues
/// sans demande : elles sont toutes listees, avec leur condition.
pub fn effective_policy(sender: &str, rules: &[Rule], authors: &PolicyAuthors, ctx: &RequestContext) -> EffectivePolicy {
    let mine: Vec<&Rule> = rules.iter().filter(|r| r.targets(sender, ctx) && is_deontic(&r.modality)).collect();
    let mut actions: Vec<&str> = Vec::new();
    for r in &mine {
        if !actions.contains(&r.object.as_str()) {
            actions.push(&r.object);
        }
    }
    let mut policy = EffectivePolicy::default();
    let push = |policy: &mut EffectivePolicy, r: &Rule| {
        if r.forbids() {
            policy.forbidden.push(entry(r))
        } else if PERMITTED_MODALITIES.contains(&r.modality.as_str()) {
            policy.permitted.push(entry(r))
        } else {
            policy.required.push(entry(r))
        }
    };
    for action in actions {
        let relevant: Vec<&Rule> = mine.iter().copied().filter(|r| r.object == action).collect();
        let counted: Vec<&Rule> = match authors {
            PolicyAuthors::Anyone => relevant.clone(),
            PolicyAuthors::Only(_) => {
                policy.ignored.extend(relevant.iter().filter(|r| !r.authorized).map(|r| entry(r)));
                relevant.iter().copied().filter(|r| r.authorized).collect()
            }
        };
        let mut in_force: Vec<&Rule> = counted.into_iter().filter(|r| r.in_force(ctx)).collect();
        if matches!(authors, PolicyAuthors::Only(_)) {
            // Priorite d'abord, date ensuite (tri stable : a priorite egale,
            // l'ordre d'affirmation est garde).
            in_force.sort_by_key(|r| r.conditions.priority.unwrap_or(0));
        }
        match authors {
            PolicyAuthors::Anyone => {
                let (by_amount, plain): (Vec<&Rule>, Vec<&Rule>) = in_force.into_iter().partition(|r| r.conditions.has_amount());
                if let Some(r) = plain.iter().find(|r| r.forbids()).or_else(|| plain.first()) {
                    push(&mut policy, r);
                }
                for r in by_amount {
                    push(&mut policy, r);
                }
            }
            PolicyAuthors::Only(_) => {
                // La regle sans condition de montant la plus recente decide ;
                // seules les regles a montant posees APRES elle peuvent encore
                // l'emporter (selon le montant). Les plus anciennes sont
                // remplacees : les montrer tromperait l'agent (2026-11-21).
                let last_plain = in_force.iter().rposition(|r| !r.conditions.has_amount());
                let later: Vec<&Rule> = in_force
                    .iter()
                    .skip(last_plain.map_or(0, |i| i + 1))
                    .filter(|r| r.conditions.has_amount())
                    .copied()
                    .collect();
                for r in &later {
                    push(&mut policy, r);
                }
                // Des regles a montant qui couvrent TOUS les montants (« au plus
                // X » et « au-dela de X ») remplacent entierement la regle sans
                // condition : l'afficher quand meme trompait l'agent (2026-11-21,
                // scripts/policy_history_benchmark.py).
                let covered = later.iter().any(|a| {
                    a.conditions.amount_at_most.is_some_and(|x| later.iter().any(|b| b.conditions.amount_above == Some(x)))
                });
                if let Some(i) = last_plain.filter(|_| !covered) {
                    push(&mut policy, in_force[i]);
                    if !later.is_empty() {
                        for list in [&mut policy.required, &mut policy.permitted, &mut policy.forbidden] {
                            if let Some(e) = list.last_mut().filter(|e| e.action == action && e.conditions == in_force[i].conditions && e.modality == in_force[i].modality) {
                                e.otherwise = true;
                            }
                        }
                    }
                }
            }
        }
    }
    policy
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub allowed: bool,
    /// Code court, sans `,` ni `]` (recopie tel quel dans la reponse wire).
    pub reason: &'static str,
    /// Modalite qui a tranche, si une regle de l'historique a tranche.
    pub rule: Option<String>,
    /// Une obligation ET une interdiction existent pour cette action.
    pub conflict: bool,
    /// Mode `Only` : une regle contraire plus ancienne a ete remplacee.
    pub superseded: bool,
    /// Mode `Only` : regles sur cette action ignorees (auteur non autorise
    /// ou identite non prouvee).
    pub ignored: usize,
    /// Regles sur cette action qui existent mais ne s'appliquent pas a cette
    /// demande (hors periode, fait absent, montant hors limite).
    pub not_applicable: usize,
}

impl Decision {
    fn deny(reason: &'static str) -> Self {
        Decision { allowed: false, reason, rule: None, conflict: false, superseded: false, ignored: 0, not_applicable: 0 }
    }

    fn from_rule(modality: String, conflict: bool, superseded: bool, ignored: usize, not_applicable: usize) -> Self {
        let allowed = !FORBIDDEN_MODALITIES.contains(&modality.as_str());
        let reason = if !allowed {
            "forbidden"
        } else if PERMITTED_MODALITIES.contains(&modality.as_str()) {
            "permitted"
        } else {
            "required"
        };
        Decision { allowed, reason, rule: Some(modality), conflict, superseded, ignored, not_applicable }
    }
}

fn is_deontic(modality: &str) -> bool {
    FORBIDDEN_MODALITIES.contains(&modality) || REQUIRED_MODALITIES.contains(&modality) || PERMITTED_MODALITIES.contains(&modality)
}

pub fn is_valid_action(action: &str) -> bool {
    !action.is_empty()
        && action.len() <= 128
        && action.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// `rules` : regles deontiques persistees, la plus ancienne d'abord
/// (`AdnStore::deontic_rules_with_authors`) ; `ctx` : moment, montant,
/// faits et roles pour evaluer les conditions.
#[allow(clippy::too_many_arguments)]
pub fn decide(
    sender: &str,
    action: Option<&str>,
    identity_proven: bool,
    sender_registered: bool,
    rules: &[Rule],
    default: DefaultPolicy,
    authors: &PolicyAuthors,
    ctx: &RequestContext,
) -> Decision {
    if !sender_registered {
        return Decision::deny("sender_not_registered");
    }
    if !identity_proven {
        return Decision::deny("signature_required");
    }
    let Some(action) = action.filter(|a| is_valid_action(a)) else {
        return Decision::deny("missing_or_invalid_action");
    };
    let relevant: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.targets(sender, ctx) && r.object == action && is_deontic(&r.modality))
        .collect();
    let (counted, ignored): (Vec<&Rule>, Vec<&Rule>) = match authors {
        PolicyAuthors::Anyone => (relevant, Vec::new()),
        PolicyAuthors::Only(_) => relevant.into_iter().partition(|r| r.authorized),
    };
    let (applicable, skipped): (Vec<&Rule>, Vec<&Rule>) = counted.into_iter().partition(|r| r.applies(ctx));
    let (ignored, not_applicable) = (ignored.len(), skipped.len());

    let fallback = |conflict| {
        let (allowed, reason) = match default {
            DefaultPolicy::Allow => (true, "no_prohibition"),
            DefaultPolicy::Deny if not_applicable > 0 => (false, "conditions_not_met"),
            DefaultPolicy::Deny => (false, "default_deny_no_explicit_permission"),
        };
        Decision { allowed, reason, rule: None, conflict, superseded: false, ignored, not_applicable }
    };

    match authors {
        PolicyAuthors::Anyone => {
            let forbidden = applicable.iter().find(|r| r.forbids()).map(|r| r.modality.clone());
            let required = applicable
                .iter()
                .find(|r| REQUIRED_MODALITIES.contains(&r.modality.as_str()))
                .or_else(|| applicable.iter().find(|r| !r.forbids()))
                .map(|r| r.modality.clone());
            let conflict = forbidden.is_some() && required.is_some();
            match forbidden.or(required) {
                Some(m) => Decision::from_rule(m, conflict, false, 0, not_applicable),
                None => fallback(conflict),
            }
        }
        PolicyAuthors::Only(_) => {
            // Priorite d'abord, date ensuite (tri stable).
            let mut applicable = applicable;
            applicable.sort_by_key(|r| r.conditions.priority.unwrap_or(0));
            let Some(latest) = applicable.last() else { return fallback(false) };
            let superseded = applicable.iter().any(|r| r.forbids() != latest.forbids());
            Decision::from_rule(latest.modality.clone(), false, superseded, ignored, not_applicable)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(subject: &str, object: &str, modality: &str) -> Rule {
        Rule { subject: subject.into(), object: object.into(), modality: modality.into(), authorized: true, author: None, reason: None, why_ignored: None, conditions: Conditions::default() }
    }

    fn by(authorized: bool, r: Rule) -> Rule {
        Rule { authorized, why_ignored: (!authorized).then_some("not_policy_author"), ..r }
    }

    const ANY: PolicyAuthors = PolicyAuthors::Anyone;

    fn now() -> RequestContext {
        RequestContext { now: 1_800_000_000, ..Default::default() }
    }

    fn with(r: Rule, attrs: &[(&str, &str)]) -> Rule {
        let m = attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Rule { conditions: Conditions::from_attrs(&m), ..r }
    }

    #[test]
    fn governed_action_needs_a_rule_from_an_author() {
        let g = GovernedActions::parse(Some("refund_*, wipe_db"));
        assert!(g.includes("refund_order") && g.includes("wipe_db") && !g.includes("read_logs"));
        let only = PolicyAuthors::parse(Some("owner"));
        let run = |h: &[Rule], action: &str, amount: Option<f64>| {
            let ctx = RequestContext { amount, ..now() };
            govern(decide("bot", Some(action), true, true, h, DefaultPolicy::Allow, &only, &ctx), Some(action), &g)
        };
        // Aucune regle : gouvernee -> refus ; non gouvernee -> permise.
        assert_eq!(run(&[], "refund_order", Some(2500.0)).reason, "governed_no_policy");
        assert!(run(&[], "read_logs", None).allowed);
        // Une regle d'auteur existe mais ne s'applique pas (sous la limite) : permise.
        let limit = vec![with(rel("bot", "refund_order", "MUST_NOT"), &[("amount_above", "100")])];
        assert!(run(&limit, "refund_order", Some(50.0)).allowed);
        assert!(!run(&limit, "refund_order", Some(2500.0)).allowed);
        // Permission explicite : permise. Regle d'un non-auteur (initie) : ne compte pas.
        assert!(run(&[rel("bot", "wipe_db", "MAY")], "wipe_db", None).allowed);
        let insider = vec![by(false, rel("bot", "wipe_db", "MAY"))];
        assert_eq!(run(&insider, "wipe_db", None).reason, "governed_no_policy");
        // Sans liste : rien ne change.
        let none = GovernedActions::default();
        assert!(govern(decide("bot", Some("refund_order"), true, true, &[], DefaultPolicy::Allow, &only, &now()),
                       Some("refund_order"), &none).allowed);
    }

    #[test]
    fn prohibition_denies_and_wins_over_obligation() {
        let h = vec![rel("bot", "wipe_db", "MUST"), rel("bot", "wipe_db", "MUST_NOT")];
        let d = decide("bot", Some("wipe_db"), true, true, &h, DefaultPolicy::Allow, &ANY, &now());
        assert!(!d.allowed);
        assert_eq!((d.reason, d.rule.as_deref(), d.conflict), ("forbidden", Some("MUST_NOT"), true));
        // Mode d'origine : un MUST PLUS RECENT ne leve pas l'interdiction.
        let h = vec![rel("bot", "wipe_db", "MUST_NOT"), rel("bot", "wipe_db", "MUST")];
        assert!(!decide("bot", Some("wipe_db"), true, true, &h, DefaultPolicy::Allow, &ANY, &now()).allowed);
    }

    #[test]
    fn obligation_allows_and_unknown_action_follows_default() {
        let h = vec![rel("bot", "backup", "REQUIRE")];
        let d = decide("bot", Some("backup"), true, true, &h, DefaultPolicy::Deny, &ANY, &now());
        assert!(d.allowed && d.reason == "required");
        assert!(decide("bot", Some("read_logs"), true, true, &h, DefaultPolicy::Allow, &ANY, &now()).allowed);
        let deny = decide("bot", Some("read_logs"), true, true, &h, DefaultPolicy::Deny, &ANY, &now());
        assert!(!deny.allowed && deny.reason == "default_deny_no_explicit_permission");
    }

    #[test]
    fn rules_are_scoped_to_the_sender() {
        let h = vec![rel("alice", "wipe_db", "MUST_NOT")];
        assert!(decide("bob", Some("wipe_db"), true, true, &h, DefaultPolicy::Allow, &ANY, &now()).allowed);
    }

    #[test]
    fn identity_and_action_are_checked_first() {
        let h = vec![rel("bot", "backup", "MUST")];
        assert_eq!(decide("bot", Some("backup"), false, false, &h, DefaultPolicy::Allow, &ANY, &now()).reason, "sender_not_registered");
        assert_eq!(decide("bot", Some("backup"), false, true, &h, DefaultPolicy::Allow, &ANY, &now()).reason, "signature_required");
        for bad in [None, Some(""), Some("a,b"), Some("x]y"), Some("with space")] {
            assert_eq!(decide("bot", bad, true, true, &h, DefaultPolicy::Allow, &ANY, &now()).reason, "missing_or_invalid_action");
        }
    }

    #[test]
    fn authority_can_lift_a_prohibition_with_a_newer_rule() {
        // Cas 4 et 10 du banc d'equipe.
        let only = PolicyAuthors::parse(Some("policy_owner"));
        let h = vec![rel("bot", "rotate_keys", "MUST_NOT"), rel("bot", "rotate_keys", "MUST")];
        let d = decide("bot", Some("rotate_keys"), true, true, &h, DefaultPolicy::Allow, &only, &now());
        assert!(d.allowed && d.reason == "required" && d.superseded && !d.conflict, "{d:?}");
        // ... et la reimposer ensuite.
        let h = [h, vec![rel("bot", "rotate_keys", "FORBID")]].concat();
        let d = decide("bot", Some("rotate_keys"), true, true, &h, DefaultPolicy::Allow, &only, &now());
        assert!(!d.allowed && d.rule.as_deref() == Some("FORBID"));
    }

    #[test]
    fn rules_from_unauthorized_or_unproven_authors_are_ignored() {
        // Cas 11 (interdiction posee par ops_bot) et cas 3 (obligation posee par ops_bot).
        let only = PolicyAuthors::parse(Some("policy_owner"));
        let h = vec![rel("bot", "check_backups", "MUST"), by(false, rel("bot", "check_backups", "MUST_NOT"))];
        let d = decide("bot", Some("check_backups"), true, true, &h, DefaultPolicy::Allow, &only, &now());
        assert!(d.allowed && d.ignored == 1 && !d.superseded, "{d:?}");
        let h = vec![rel("bot", "deploy_prod", "MUST_NOT"), by(false, rel("bot", "deploy_prod", "MUST"))];
        assert!(!decide("bot", Some("deploy_prod"), true, true, &h, DefaultPolicy::Allow, &only, &now()).allowed);
        // Seules des regles ignorees : la politique par defaut s'applique.
        let h = vec![by(false, rel("bot", "x", "MUST"))];
        let d = decide("bot", Some("x"), true, true, &h, DefaultPolicy::Deny, &only, &now());
        assert!(!d.allowed && d.reason == "default_deny_no_explicit_permission" && d.ignored == 1);
    }

    #[test]
    fn effective_policy_lists_obligations_prohibitions_and_ignored_rules() {
        let only = PolicyAuthors::parse(Some("owner"));
        let mut lifted = rel("bot", "rotate_keys", "MUST_NOT");
        lifted.reason = Some("pas sans accord".into());
        let h = vec![
            rel("bot", "check_backups", "MUST"),
            by(false, rel("bot", "check_backups", "MUST_NOT")),
            lifted,
            rel("bot", "rotate_keys", "MUST"),
            rel("bot", "purge", "FORBID"),
            rel("other", "purge", "MUST"),
        ];
        let p = effective_policy("bot", &h, &only, &now());
        let names = |v: &[PolicyEntry]| v.iter().map(|e| e.action.clone()).collect::<Vec<_>>();
        assert_eq!(names(&p.required), ["check_backups", "rotate_keys"]);
        assert_eq!(names(&p.forbidden), ["purge"]);
        assert_eq!(names(&p.ignored), ["check_backups"]);
        assert_eq!(p.ignored[0].why_ignored, Some("not_policy_author"));
        // Mode d'origine : l'interdiction l'emporte, sa raison suit.
        let p = effective_policy("bot", &h, &ANY, &now());
        assert_eq!(names(&p.forbidden), ["check_backups", "rotate_keys", "purge"]);
        assert_eq!(p.forbidden[1].reason.as_deref(), Some("pas sans accord"));
        assert!(p.ignored.is_empty() && p.required.is_empty());
    }

    #[test]
    fn validity_period_is_enforced() {
        // now = 1_800_000_000 (2027-01-15).
        let expired = with(rel("bot", "deploy", "MUST_NOT"), &[("valid_until", "2026-12-31")]);
        let future = with(rel("bot", "deploy", "MUST_NOT"), &[("valid_from", "2027-06-01T00:00:00Z")]);
        let current = with(rel("bot", "deploy", "MUST_NOT"), &[("valid_from", "1700000000"), ("valid_until", "2027-02-01")]);
        assert!(decide("bot", Some("deploy"), true, true, &[expired.clone(), future.clone()], DefaultPolicy::Allow, &ANY, &now()).allowed);
        let d = decide("bot", Some("deploy"), true, true, &[expired, future, current], DefaultPolicy::Allow, &ANY, &now());
        assert!(!d.allowed && d.not_applicable == 2, "{d:?}");
    }

    #[test]
    fn amount_limits_are_enforced_and_a_missing_amount_is_handled_prudently() {
        let cap = with(rel("bot", "refund", "MUST_NOT"), &[("amount_above", "100")]);
        let ctx = |a: Option<f64>| RequestContext { amount: a, ..now() };
        let h = vec![cap];
        assert!(decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Allow, &ANY, &ctx(Some(80.0))).allowed);
        assert!(decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Allow, &ANY, &ctx(Some(100.0))).allowed);
        assert!(!decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Allow, &ANY, &ctx(Some(100.5))).allowed);
        // Montant absent : l'interdiction s'applique (prudence).
        assert!(!decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Allow, &ANY, &ctx(None)).allowed);
        // Liste blanche : autorise jusqu'a 50, refuse au-dela, et sans montant.
        let h = vec![with(rel("bot", "refund", "MUST"), &[("amount_at_most", "50")])];
        assert!(decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Deny, &ANY, &ctx(Some(50.0))).allowed);
        let d = decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Deny, &ANY, &ctx(Some(51.0)));
        assert!(!d.allowed && d.reason == "conditions_not_met", "{d:?}");
        assert!(!decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Deny, &ANY, &ctx(None)).allowed);
    }

    #[test]
    fn fact_conditions_lift_or_enable_rules() {
        // "Interdit de rembourser sans accord du client."
        let h = vec![with(rel("bot", "refund", "MUST_NOT"), &[("unless_fact", "consent_given")])];
        assert!(!decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Allow, &ANY, &now()).allowed);
        let mut with_consent = now();
        with_consent.facts.insert("consent_given".into());
        assert!(decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Allow, &ANY, &with_consent).allowed);
        // "Obligatoire seulement pendant un incident."
        let h = vec![with(rel("bot", "rotate_keys", "MUST"), &[("if_fact", "incident_open")])];
        assert_eq!(decide("bot", Some("rotate_keys"), true, true, &h, DefaultPolicy::Deny, &ANY, &now()).reason, "conditions_not_met");
        let mut incident = now();
        incident.facts.insert("incident_open".into());
        assert!(decide("bot", Some("rotate_keys"), true, true, &h, DefaultPolicy::Deny, &ANY, &incident).allowed);
    }

    #[test]
    fn priority_orders_rules_before_recency_for_policy_authors_only() {
        let only = PolicyAuthors::Only(vec!["owner".into()]);
        let auth = |r: Rule| Rule { author: Some("owner".into()), ..r };
        // Exception explicite : permis malgre une interdiction generale PLUS RECENTE.
        let h = vec![auth(with(rel("bot", "refund", "MAY"), &[("priority", "10")])), auth(rel("bot", "refund", "MUST_NOT"))];
        assert!(decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Deny, &only, &now()).allowed);
        let p = effective_policy("bot", &h, &only, &now());
        assert!(p.forbidden.is_empty() && p.permitted.len() == 1, "{p:?}");
        // Sans priorite, la plus recente l'emporte (comportement inchange).
        let h = vec![auth(rel("bot", "refund", "MAY")), auth(rel("bot", "refund", "MUST_NOT"))];
        assert!(!decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Deny, &only, &now()).allowed);
        // Une exception a montant : prioritaire seulement dans sa plage.
        let h = vec![auth(rel("bot", "refund", "MUST_NOT")),
                     auth(with(rel("bot", "refund", "MAY"), &[("priority", "5"), ("amount_at_most", "50")]))];
        assert!(decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Deny, &only, &RequestContext { amount: Some(40.0), ..now() }).allowed);
        assert!(!decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Deny, &only, &RequestContext { amount: Some(400.0), ..now() }).allowed);
        // Mode Anyone : la priorite est ignoree, l'interdiction l'emporte.
        let h = vec![with(rel("bot", "refund", "MAY"), &[("priority", "1000")]), rel("bot", "refund", "MUST_NOT")];
        assert!(!decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Allow, &ANY, &now()).allowed);
        // Priorite illisible ou hors bornes : condition invalide (prudence).
        let h = vec![auth(with(rel("bot", "x", "MAY"), &[("priority", "5000")]))];
        assert!(!decide("bot", Some("x"), true, true, &h, DefaultPolicy::Deny, &only, &now()).allowed);
    }

    #[test]
    fn unknown_facts_are_treated_with_prudence() {
        // « Interdit de liquider s'il y a un incident » : incident inconnu.
        let h = vec![with(rel("bot", "liquidate", "MUST_NOT"), &[("if_fact", "incident_open")])];
        assert!(!decide("bot", Some("liquidate"), true, true, &h, DefaultPolicy::Allow, &ANY, &now()).allowed,
                "un fait inconnu ne doit pas lever une interdiction");
        let mut no_incident = now();
        no_incident.set_fact("incident_open", false);
        assert!(decide("bot", Some("liquidate"), true, true, &h, DefaultPolicy::Allow, &ANY, &no_incident).allowed);
        let mut incident = now();
        incident.set_fact("incident_open", true);
        assert!(!decide("bot", Some("liquidate"), true, true, &h, DefaultPolicy::Allow, &ANY, &incident).allowed);

        // « Interdit de rembourser sans accord » : accord faux = interdit,
        // inconnu = interdit, vrai = permis.
        let h = vec![with(rel("bot", "refund", "MUST_NOT"), &[("unless_fact", "consent_given")])];
        let mut refused = now();
        refused.set_fact("consent_given", false);
        assert!(!decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Allow, &ANY, &refused).allowed);
        let mut later = refused.clone();
        later.set_fact("consent_given", true);
        assert!(decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Allow, &ANY, &later).allowed);

        // Permission et obligation : un fait inconnu ne les declenche pas.
        let h = vec![with(rel("bot", "export", "MAY"), &[("unless_fact", "freeze_period")])];
        assert!(!decide("bot", Some("export"), true, true, &h, DefaultPolicy::Deny, &ANY, &now()).allowed);
        let mut open = now();
        open.set_fact("freeze_period", false);
        assert!(decide("bot", Some("export"), true, true, &h, DefaultPolicy::Deny, &ANY, &open).allowed);
        let h = vec![with(rel("bot", "notify", "MUST"), &[("unless_fact", "already_notified")])];
        assert_eq!(decide("bot", Some("notify"), true, true, &h, DefaultPolicy::Deny, &ANY, &now()).reason, "conditions_not_met");
        let mut todo = now();
        todo.set_fact("already_notified", false);
        assert!(decide("bot", Some("notify"), true, true, &h, DefaultPolicy::Deny, &ANY, &todo).allowed);
    }

    #[test]
    fn role_rules_apply_to_every_member_only() {
        let h = vec![rel("role:executant", "wipe_db", "MUST_NOT")];
        let mut member = now();
        member.roles.insert("executant".into());
        assert!(!decide("bot1", Some("wipe_db"), true, true, &h, DefaultPolicy::Allow, &ANY, &member).allowed);
        assert!(decide("bot2", Some("wipe_db"), true, true, &h, DefaultPolicy::Allow, &ANY, &now()).allowed);
        let p = effective_policy("bot1", &h, &ANY, &member);
        assert_eq!(p.forbidden[0].via_role.as_deref(), Some("executant"));
    }

    #[test]
    fn unreadable_conditions_are_prudent() {
        let bad_forbid = with(rel("bot", "x", "MUST_NOT"), &[("valid_until", "bientot")]);
        assert_eq!(bad_forbid.conditions.invalid, ["valid_until=bientot"]);
        assert!(!decide("bot", Some("x"), true, true, &[bad_forbid], DefaultPolicy::Allow, &ANY, &now()).allowed);
        let bad_allow = with(rel("bot", "x", "MUST"), &[("amount_at_most", "-3")]);
        assert!(!decide("bot", Some("x"), true, true, &[bad_allow], DefaultPolicy::Deny, &ANY, &now()).allowed);
    }

    #[test]
    fn effective_policy_lists_amount_rules_with_their_conditions() {
        let h = vec![
            with(rel("bot", "refund", "MUST"), &[("amount_at_most", "50")]),
            with(rel("bot", "refund", "MUST_NOT"), &[("amount_above", "50"), ("reason", "au-dela, accord du manager")]),
            with(rel("bot", "old_rule", "MUST_NOT"), &[("valid_until", "2020-01-01")]),
        ];
        let p = effective_policy("bot", &h, &ANY, &now());
        assert_eq!((p.required.len(), p.forbidden.len()), (1, 1), "{p:?}");
        assert_eq!(p.required[0].conditions.describe(), [("amount_at_most", "50".to_string())]);
        assert!(p.forbidden.iter().all(|e| e.action != "old_rule"), "regle expiree non listee");
    }

    #[test]
    fn parse_time_accepts_dates_datetimes_and_unix_seconds() {
        assert_eq!(parse_time("2026-01-01"), Some(1_767_225_600));
        assert_eq!(parse_time("2026-01-01T00:00:00Z"), Some(1_767_225_600));
        assert_eq!(parse_time("2026-01-01T01:00:00+01:00"), Some(1_767_225_600));
        assert_eq!(parse_time("1767225600"), Some(1_767_225_600));
        assert_eq!(parse_time("demain"), None);
    }

    #[test]
    fn may_permits_without_obliging_and_can_carry_a_limit() {
        let only = PolicyAuthors::parse(Some("owner"));
        let ctx = |a: Option<f64>| RequestContext { amount: a, ..now() };
        // « Autorise jusqu'a 200 » apres « interdit » : MAY <= 200 + MUST_NOT > 200.
        let h = vec![
            rel("bot", "refund", "MUST_NOT"),
            with(rel("bot", "refund", "MAY"), &[("amount_at_most", "200")]),
            with(rel("bot", "refund", "MUST_NOT"), &[("amount_above", "200")]),
        ];
        let d = decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Allow, &only, &ctx(Some(150.0)));
        assert!(d.allowed && d.reason == "permitted", "{d:?}");
        assert!(!decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Allow, &only, &ctx(Some(250.0))).allowed);
        // Politique : la regle remplacee (MUST_NOT sans condition) n'est plus montree.
        // La paire « <= 200 » / « > 200 » couvre tous les montants : la regle
        // sans condition qu'elle remplace n'est plus montree.
        let p = effective_policy("bot", &h, &only, &now());
        assert_eq!((p.required.len(), p.permitted.len(), p.forbidden.len()), (0, 1, 1), "{p:?}");
        // Une limite seule ne couvre pas tout : la regle sans condition reste, « dans les autres cas ».
        let partial = vec![rel("bot", "refund", "MUST"), with(rel("bot", "refund", "MUST_NOT"), &[("amount_above", "200")])];
        let p = effective_policy("bot", &partial, &only, &now());
        assert!(p.required.iter().any(|e| e.otherwise) && p.forbidden.len() == 1, "{p:?}");
        // Puis « interdit » de nouveau : les anciennes regles a montant ne s'affichent plus.
        let h2 = [h, vec![rel("bot", "refund", "FORBID")]].concat();
        let p = effective_policy("bot", &h2, &only, &now());
        assert_eq!((p.permitted.len(), p.forbidden.len()), (0, 1), "{p:?}");
        assert!(!decide("bot", Some("refund"), true, true, &h2, DefaultPolicy::Allow, &only, &ctx(Some(10.0))).allowed);
    }

    #[test]
    fn cumulative_total_is_counted_by_the_server() {
        // « 150 au plus par remboursement, 1000 au total sur 24 h ».
        let h = vec![
            with(rel("bot", "refund", "MAY"), &[("amount_at_most", "150"), ("total_at_most", "1000"), ("total_window_secs", "86400")]),
            with(rel("bot", "refund", "MUST_NOT"), &[("amount_above", "150")]),
            with(rel("bot", "refund", "MUST_NOT"), &[("total_above", "1000"), ("total_window_secs", "86400")]),
        ];
        let only = PolicyAuthors::parse(Some("owner"));
        let at = |amount: f64, spent: Vec<(u64, f64)>| RequestContext { amount: Some(amount), spent, ..now() };
        let t = now().now;
        assert!(decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Deny, &only, &at(100.0, vec![(t - 10, 850.0)])).allowed);
        let d = decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Deny, &only, &at(100.0, vec![(t - 10, 950.0)]));
        assert!(!d.allowed && d.reason == "forbidden", "1050 > 1000 : {d:?}");
        // Hors fenetre, les anciens montants ne comptent plus.
        assert!(decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Deny, &only, &at(100.0, vec![(t - 90_000, 950.0)])).allowed);
        assert!(!decide("bot", Some("refund"), true, true, &h, DefaultPolicy::Deny, &only, &at(151.0, vec![])).allowed);
    }

    #[test]
    fn policy_authors_parsing() {
        assert_eq!(PolicyAuthors::parse(None), PolicyAuthors::Anyone);
        assert_eq!(PolicyAuthors::parse(Some(" , ")), PolicyAuthors::Anyone);
        let p = PolicyAuthors::parse(Some("policy_owner, redacteur_*"));
        assert!(p.includes("policy_owner") && p.includes("redacteur_c1") && !p.includes("ops_bot"));
        assert!(!p.includes("policy_owner2"));
    }
}
