// -*- coding: utf-8 -*-
// ╔══════════════════════════════════════════════════════════════════════╗
// ║  CSTL — Protocole Réseau                                           ║
// ║  Communication inter-agents via fichiers JSON                      ║
// ║  Sérialisation · Fusion Ω_net · Dialogue réel                      ║
// ╚══════════════════════════════════════════════════════════════════════╝
//
// COMPILATION :
//   g++ -std=c++17 -O2 -finput-charset=UTF-8 -o cstl_agent cstl_network.cpp
//
// USAGE :
//   Terminal 1 : ./cstl_agent alpha    (IA_Alpha envoie)
//   Terminal 2 : ./cstl_agent beta     (IA_Beta reçoit, répond)
//   Terminal 3 : ./cstl_agent alpha    (IA_Alpha lit la réponse)
//   OU en une commande :
//   ./cstl_agent demo                  (simulation complète)

#include <algorithm>
#include <chrono>
#include <ctime>
#include <fstream>
#include <functional>
#include <iomanip>
#include <iostream>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <thread>
#include <unordered_map>
#include <vector>
#include <cmath>

using namespace std;

// ══════════════════════════════════════════════════════════════════════
// SECTION 1 — TYPES CSTL
// ══════════════════════════════════════════════════════════════════════

enum class TimeOp   { PAST, PRESENT, FUTURE, INTRICATED };
enum class Polarity { POSITIVE, NEGATIVE, NEUTRAL };
enum class Dynamic  { STRENGTHEN, WEAKEN, NONE };
enum class RelType  { CAUSAL, MUTUAL, TENSION };

string timeop_str(TimeOp t) {
    switch(t) {
        case TimeOp::PAST:       return "PAST";
        case TimeOp::FUTURE:     return "FUTURE";
        case TimeOp::INTRICATED: return "INTRICATED";
        default:                 return "PRESENT";
    }
}
TimeOp str_timeop(const string& s) {
    if (s=="PAST")       return TimeOp::PAST;
    if (s=="FUTURE")     return TimeOp::FUTURE;
    if (s=="INTRICATED") return TimeOp::INTRICATED;
    return TimeOp::PRESENT;
}
string polarity_str(Polarity p) {
    switch(p) {
        case Polarity::POSITIVE: return "POSITIVE";
        case Polarity::NEGATIVE: return "NEGATIVE";
        default:                 return "NEUTRAL";
    }
}
Polarity str_polarity(const string& s) {
    if (s=="POSITIVE") return Polarity::POSITIVE;
    if (s=="NEGATIVE") return Polarity::NEGATIVE;
    return Polarity::NEUTRAL;
}
string dynamic_str(Dynamic d) {
    switch(d) {
        case Dynamic::STRENGTHEN: return "STRENGTHEN";
        case Dynamic::WEAKEN:     return "WEAKEN";
        default:                  return "NONE";
    }
}
Dynamic str_dynamic(const string& s) {
    if (s=="STRENGTHEN") return Dynamic::STRENGTHEN;
    if (s=="WEAKEN")     return Dynamic::WEAKEN;
    return Dynamic::NONE;
}
string reltype_str(RelType r) {
    switch(r) {
        case RelType::MUTUAL:  return "MUTUAL";
        case RelType::TENSION: return "TENSION";
        default:               return "CAUSAL";
    }
}
RelType str_reltype(const string& s) {
    if (s=="MUTUAL")  return RelType::MUTUAL;
    if (s=="TENSION") return RelType::TENSION;
    return RelType::CAUSAL;
}

struct CSTLRelation {
    string   from, to, action;
    Polarity polarity = Polarity::POSITIVE;
    Dynamic  dynamic  = Dynamic::NONE;
    TimeOp   time_op  = TimeOp::PRESENT;
    RelType  type     = RelType::CAUSAL;
    double   weight   = 1.0;
    int      count    = 1;

    string key() const { return from + "|" + action + "|" + to; }

    string time_sym() const {
        return time_op==TimeOp::PAST       ? "<<"   :
               time_op==TimeOp::FUTURE     ? ">>"   :
               time_op==TimeOp::INTRICATED ? "<<=>>" : "=";
    }
    string pol_sym() const {
        return polarity==Polarity::POSITIVE ? "+" :
               polarity==Polarity::NEGATIVE ? "-" : "o";
    }
    string dyn_sym() const {
        return dynamic==Dynamic::STRENGTHEN ? "^" :
               dynamic==Dynamic::WEAKEN     ? "v" : "";
    }
    string display() const {
        return time_sym() + "  ." + from
             + "  ->" + pol_sym() + dyn_sym()
             + "  " + action
             + "  ." + to;
    }
};

struct CSTLNode {
    string id, label;
    bool   is_meta = false;
    double density = 0.0;
};

// ══════════════════════════════════════════════════════════════════════
// SECTION 2 — JSON MINIMAL (sans dépendance externe)
// ══════════════════════════════════════════════════════════════════════

// Échapper les caractères spéciaux JSON
string json_escape(const string& s) {
    string out;
    for (char c : s) {
        if      (c == '"')  out += "\\\"";
        else if (c == '\\') out += "\\\\";
        else if (c == '\n') out += "\\n";
        else if (c == '\r') out += "\\r";
        else if (c == '\t') out += "\\t";
        else out += c;
    }
    return out;
}

// Extraire la valeur d'une clé string dans un JSON brut
string json_get_str(const string& json, const string& key) {
    string pat = "\"" + key + "\"";
    size_t p = json.find(pat);
    if (p == string::npos) return "";
    size_t q = json.find(':', p + pat.size());
    if (q == string::npos) return "";
    size_t s = json.find('"', q + 1);
    if (s == string::npos) return "";
    size_t e = s + 1;
    while (e < json.size()) {
        if (json[e] == '\\') { e += 2; continue; }
        if (json[e] == '"')  break;
        ++e;
    }
    return json.substr(s + 1, e - s - 1);
}

// Extraire un tableau JSON d'objets [{...},{...}]
vector<string> json_get_array(const string& json, const string& key) {
    vector<string> items;
    string pat = "\"" + key + "\"";
    size_t p = json.find(pat);
    if (p == string::npos) return items;
    size_t q = json.find('[', p);
    if (q == string::npos) return items;
    int depth = 0;
    size_t obj_start = string::npos;
    bool in_str = false;
    for (size_t i = q; i < json.size(); ++i) {
        char c = json[i];
        if (c == '"' && (i == 0 || json[i-1] != '\\')) { in_str = !in_str; continue; }
        if (in_str) continue;
        if (c == '{') {
            if (depth == 1) obj_start = i;
            ++depth;
        } else if (c == '}') {
            --depth;
            if (depth == 1 && obj_start != string::npos) {
                items.push_back(json.substr(obj_start, i - obj_start + 1));
                obj_start = string::npos;
            }
        } else if (c == '[') {
            if (i == q) depth = 1;
            else ++depth;
        } else if (c == ']') {
            if (depth == 1) break;
            --depth;
        }
    }
    return items;
}

// Extraire un sous-objet JSON { ... }
string json_get_obj(const string& json, const string& key) {
    string pat = "\"" + key + "\"";
    size_t p = json.find(pat);
    if (p == string::npos) return "{}";
    size_t q = json.find('{', p);
    if (q == string::npos) return "{}";
    int depth = 0;
    bool in_str = false;
    for (size_t i = q; i < json.size(); ++i) {
        char c = json[i];
        if (c == '"' && (i == 0 || json[i-1] != '\\')) { in_str = !in_str; continue; }
        if (in_str) continue;
        if (c == '{') ++depth;
        else if (c == '}') {
            --depth;
            if (depth == 0) return json.substr(q, i - q + 1);
        }
    }
    return "{}";
}

// ══════════════════════════════════════════════════════════════════════
// SECTION 3 — SCHEMA CSTL (sérialisable)
// ══════════════════════════════════════════════════════════════════════

struct CSTLSchema {
    string                      id;
    string                      agent_id;      // qui a créé ce schema
    string                      timestamp;
    string                      summary;
    string                      psi_intention;
    string                      psi_emotion;
    string                      psi_tone;
    vector<CSTLNode>            nodes;
    vector<CSTLRelation>        relations;
    map<string,string>          dict;          // label -> definition
    vector<string>              delta_log;
    int                         version = 1;

    // ── Sérialisation JSON ────────────────────────────────────────────
    string to_json() const {
        ostringstream ss;
        ss << "{\n";
        ss << "  \"id\": \"" << json_escape(id) << "\",\n";
        ss << "  \"agent_id\": \"" << json_escape(agent_id) << "\",\n";
        ss << "  \"timestamp\": \"" << json_escape(timestamp) << "\",\n";
        ss << "  \"version\": " << version << ",\n";
        ss << "  \"summary\": \"" << json_escape(summary) << "\",\n";
        ss << "  \"psi_intention\": \"" << json_escape(psi_intention) << "\",\n";
        ss << "  \"psi_emotion\": \"" << json_escape(psi_emotion) << "\",\n";
        ss << "  \"psi_tone\": \"" << json_escape(psi_tone) << "\",\n";

        // Nodes
        ss << "  \"nodes\": [\n";
        for (size_t i = 0; i < nodes.size(); ++i) {
            auto& n = nodes[i];
            ss << "    {\"id\": \"" << json_escape(n.id)
               << "\", \"label\": \"" << json_escape(n.label)
               << "\", \"is_meta\": " << (n.is_meta ? "true" : "false")
               << ", \"density\": " << fixed << setprecision(3) << n.density
               << "}";
            if (i + 1 < nodes.size()) ss << ",";
            ss << "\n";
        }
        ss << "  ],\n";

        // Relations
        ss << "  \"relations\": [\n";
        for (size_t i = 0; i < relations.size(); ++i) {
            auto& r = relations[i];
            ss << "    {\"from\": \"" << json_escape(r.from)
               << "\", \"to\": \"" << json_escape(r.to)
               << "\", \"action\": \"" << json_escape(r.action)
               << "\", \"polarity\": \"" << polarity_str(r.polarity)
               << "\", \"dynamic\": \"" << dynamic_str(r.dynamic)
               << "\", \"time_op\": \"" << timeop_str(r.time_op)
               << "\", \"type\": \"" << reltype_str(r.type)
               << "\", \"weight\": " << fixed << setprecision(3) << r.weight
               << ", \"count\": " << r.count
               << "}";
            if (i + 1 < relations.size()) ss << ",";
            ss << "\n";
        }
        ss << "  ],\n";

        // Dict
        ss << "  \"dict\": {\n";
        size_t di = 0;
        for (auto& [lbl, def] : dict) {
            ss << "    \"" << json_escape(lbl) << "\": \""
               << json_escape(def) << "\"";
            if (++di < dict.size()) ss << ",";
            ss << "\n";
        }
        ss << "  },\n";

        // Delta log
        ss << "  \"delta_log\": [\n";
        for (size_t i = 0; i < delta_log.size(); ++i) {
            ss << "    \"" << json_escape(delta_log[i]) << "\"";
            if (i + 1 < delta_log.size()) ss << ",";
            ss << "\n";
        }
        ss << "  ]\n";
        ss << "}\n";
        return ss.str();
    }

    // ── Désérialisation JSON ──────────────────────────────────────────
    static CSTLSchema from_json(const string& json) {
        CSTLSchema s;
        s.id            = json_get_str(json, "id");
        s.agent_id      = json_get_str(json, "agent_id");
        s.timestamp     = json_get_str(json, "timestamp");
        s.summary       = json_get_str(json, "summary");
        s.psi_intention = json_get_str(json, "psi_intention");
        s.psi_emotion   = json_get_str(json, "psi_emotion");
        s.psi_tone      = json_get_str(json, "psi_tone");

        // Nodes
        for (auto& nj : json_get_array(json, "nodes")) {
            CSTLNode n;
            n.id      = json_get_str(nj, "id");
            n.label   = json_get_str(nj, "label");
            n.is_meta = (json_get_str(nj, "is_meta") == "true");
            try { n.density = stod(json_get_str(nj, "density")); } catch(...) {}
            if (!n.id.empty()) s.nodes.push_back(n);
        }

        // Relations
        for (auto& rj : json_get_array(json, "relations")) {
            CSTLRelation r;
            r.from     = json_get_str(rj, "from");
            r.to       = json_get_str(rj, "to");
            r.action   = json_get_str(rj, "action");
            r.polarity = str_polarity(json_get_str(rj, "polarity"));
            r.dynamic  = str_dynamic(json_get_str(rj, "dynamic"));
            r.time_op  = str_timeop(json_get_str(rj, "time_op"));
            r.type     = str_reltype(json_get_str(rj, "type"));
            try { r.weight = stod(json_get_str(rj, "weight")); } catch(...) {}
            try { r.count  = stoi(json_get_str(rj, "count"));  } catch(...) {}
            if (!r.from.empty() && !r.to.empty()) s.relations.push_back(r);
        }

        // Dict — parser les paires clé:valeur
        string dict_obj = json_get_obj(json, "dict");
        size_t pos = 0;
        while (pos < dict_obj.size()) {
            size_t ks = dict_obj.find('"', pos);
            if (ks == string::npos) break;
            size_t ke = dict_obj.find('"', ks + 1);
            if (ke == string::npos) break;
            string key = dict_obj.substr(ks + 1, ke - ks - 1);
            size_t colon = dict_obj.find(':', ke + 1);
            if (colon == string::npos) break;
            size_t vs = dict_obj.find('"', colon + 1);
            if (vs == string::npos) break;
            size_t ve = vs + 1;
            while (ve < dict_obj.size()) {
                if (dict_obj[ve] == '\\') { ve += 2; continue; }
                if (dict_obj[ve] == '"')  break;
                ++ve;
            }
            string val = dict_obj.substr(vs + 1, ve - vs - 1);
            if (!key.empty() && key != "dict") s.dict[key] = val;
            pos = ve + 1;
        }

        // Delta log
        for (auto& dj : json_get_array(json, "delta_log")) {
            // dj est une string JSON = "..."
            if (dj.size() >= 2) s.delta_log.push_back(dj.substr(1, dj.size()-2));
        }

        return s;
    }
};

// ══════════════════════════════════════════════════════════════════════
// SECTION 4 — OMEGA_NET RÉSEAU (avec persistance fichier)
// ══════════════════════════════════════════════════════════════════════

struct NetworkOmega {
    string                       agent_id;
    map<string, CSTLSchema>      schemas;       // id -> schema
    map<string, string>          global_dict;   // label -> definition
    map<string, string>          dict_owner;    // label -> agent
    map<string, int>             dict_conflicts;// label -> nb conflits
    vector<string>               net_log;
    string                       inbox_file;    // où on lit les messages
    string                       outbox_file;   // où on écrit

    NetworkOmega(const string& agent, const string& basedir = ".") {
        agent_id    = agent;
        inbox_file  = basedir + "/cstl_msg_to_" + agent + ".json";
        outbox_file = basedir + "/cstl_msg_from_" + agent + ".json";
    }

    // Timestamp ISO simple
    static string now_str() {
        auto t = time(nullptr);
        char buf[64];
        strftime(buf, sizeof(buf), "%Y-%m-%dT%H:%M:%S", localtime(&t));
        return string(buf);
    }

    void log(const string& msg) {
        string entry = "[" + now_str() + "][" + agent_id + "] " + msg;
        net_log.push_back(entry);
        cout << entry << "\n";
    }

    // ── Envoyer un schema à un autre agent ────────────────────────────
    bool send_schema(const CSTLSchema& s, const string& target_agent,
                     const string& basedir = ".") {
        string target_inbox = basedir + "/cstl_msg_to_" + target_agent + ".json";
        ofstream f(target_inbox);
        if (!f) {
            log("ERREUR: impossible d'écrire vers " + target_inbox);
            return false;
        }
        f << s.to_json();
        log(">> ENVOI schema [" + s.id + "] vers " + target_agent);
        log("   Fichier : " + target_inbox);
        log("   Noeuds  : " + to_string(s.nodes.size()));
        log("   Rels    : " + to_string(s.relations.size()));
        log("   Dict    : " + to_string(s.dict.size()) + " entrees");
        return true;
    }

    // ── Recevoir un schema depuis l'inbox ─────────────────────────────
    bool receive_schema(CSTLSchema& out) {
        ifstream f(inbox_file);
        if (!f) {
            log("-- inbox vide ou inexistante : " + inbox_file);
            return false;
        }
        string json((istreambuf_iterator<char>(f)),
                     istreambuf_iterator<char>());
        if (json.empty()) {
            log("-- inbox vide");
            return false;
        }
        out = CSTLSchema::from_json(json);
        log("<< RECU schema [" + out.id + "] de " + out.agent_id);
        log("   Noeuds  : " + to_string(out.nodes.size()));
        log("   Rels    : " + to_string(out.relations.size()));
        log("   Dict    : " + to_string(out.dict.size()) + " entrees");
        // Supprimer le message traité
        remove(inbox_file.c_str());
        return true;
    }

    // ── Fusionner un schema reçu dans le Omega_net local ─────────────
    void merge_schema(const CSTLSchema& incoming) {
        // 1. Enregistrer le schema
        schemas[incoming.id] = incoming;
        log("MERGE schema [" + incoming.id + "]");

        // 2. Fusionner le DICT — détecter les divergences
        int new_entries = 0, conflicts = 0, enriched = 0;
        for (auto& [lbl, def] : incoming.dict) {
            if (!global_dict.count(lbl)) {
                global_dict[lbl]  = def;
                dict_owner[lbl]   = incoming.agent_id;
                ++new_entries;
            } else if (global_dict[lbl] != def) {
                // Divergence — garder les deux, signaler
                dict_conflicts[lbl]++;
                log("!! DIVERGENCE DICT[" + lbl + "]");
                log("   Stocke  : \"" + global_dict[lbl] + "\"");
                log("   Recu    : \"" + def + "\"");
                log("   Action  : merge (concatenation)");
                global_dict[lbl] += " | " + def;
                ++conflicts;
            } else {
                ++enriched;
            }
        }
        log("   DICT : +" + to_string(new_entries) + " nouveaux, "
            + to_string(conflicts) + " conflits, "
            + to_string(enriched) + " confirmes");
    }

    // ── Afficher l'état du Omega_net ──────────────────────────────────
    void print_status() const {
        cout << "\n+-- Omega_net [" << agent_id << "] --+\n";
        cout << "  Schemas  : " << schemas.size() << "\n";
        cout << "  Dict     : " << global_dict.size() << " entrees\n";
        cout << "  Conflits : " << dict_conflicts.size() << "\n";
        if (!global_dict.empty()) {
            cout << "\n  DICT global :\n";
            for (auto& [lbl, def] : global_dict)
                cout << "    ." << lbl << " -> \"" << def.substr(0, 60)
                     << (def.size() > 60 ? "..." : "") << "\"\n";
        }
    }

    // ── Sauvegarder l'état Omega_net sur disque ───────────────────────
    void save_state(const string& path) const {
        ofstream f(path);
        if (!f) return;
        f << "{\n";
        f << "  \"agent_id\": \"" << agent_id << "\",\n";
        f << "  \"timestamp\": \"" << now_str() << "\",\n";
        f << "  \"dict_size\": " << global_dict.size() << ",\n";
        f << "  \"schemas_count\": " << schemas.size() << ",\n";
        f << "  \"global_dict\": {\n";
        size_t i = 0;
        for (auto& [lbl, def] : global_dict) {
            f << "    \"" << json_escape(lbl) << "\": \""
              << json_escape(def) << "\"";
            if (++i < global_dict.size()) f << ",";
            f << "\n";
        }
        f << "  }\n}\n";
        cout << "  [Omega_net sauvegarde -> " << path << "]\n";
    }
};

// ══════════════════════════════════════════════════════════════════════
// SECTION 5 — AGENTS CSTL PRÉDÉFINIS
// ══════════════════════════════════════════════════════════════════════

// IA_Alpha — détecte une anomalie dans un système distribué
CSTLSchema build_schema_alpha() {
    CSTLSchema s;
    s.id            = "SCHEMA_ALPHA_001";
    s.agent_id      = "IA_Alpha";
    s.timestamp     = NetworkOmega::now_str();
    s.version       = 1;
    s.summary       = "Anomalie detectee : flux degrade -> performance critique -> intervention requise";
    s.psi_intention = "ALERTE";
    s.psi_emotion   = "TENSION";
    s.psi_tone      = "ASSERTIF+URGENT";

    // Noeuds
    for (auto& [id,lbl] : vector<pair<string,string>>{
        {"n0","flux_donnees"}, {"n1","taux_erreur"}, {"n2","performance"},
        {"n3","seuil_critique"}, {"n4","intervention"}, {"n5","stabilisation"}
    }) s.nodes.push_back({id, lbl, false, 0.0});

    // Relations
    auto mk = [](const string& f, const string& t, const string& act,
                 Polarity p, Dynamic d, TimeOp to) {
        CSTLRelation r;
        r.from=f; r.to=t; r.action=act;
        r.polarity=p; r.dynamic=d; r.time_op=to;
        return r;
    };
    s.relations.push_back(mk("n0","n1","AUGMENTE",  Polarity::POSITIVE,Dynamic::STRENGTHEN,TimeOp::PAST));
    s.relations.push_back(mk("n1","n2","DETERIORE", Polarity::NEGATIVE,Dynamic::WEAKEN,   TimeOp::PRESENT));
    s.relations.push_back(mk("n2","n3","APPROCHE",  Polarity::POSITIVE,Dynamic::STRENGTHEN,TimeOp::PRESENT));
    s.relations.push_back(mk("n3","n4","REQUIERT",  Polarity::POSITIVE,Dynamic::NONE,     TimeOp::FUTURE));
    s.relations.push_back(mk("n4","n5","PRODUIT",   Polarity::POSITIVE,Dynamic::STRENGTHEN,TimeOp::FUTURE));
    // Boucle : stabilisation -> flux_donnees -> taux_erreur
    s.relations.push_back(mk("n5","n0","RESTAURE",  Polarity::POSITIVE,Dynamic::NONE,     TimeOp::FUTURE));

    // Dict sémantique
    s.dict["flux_donnees"]   = "volume de donnees traitees par unite de temps";
    s.dict["taux_erreur"]    = "frequence des erreurs dans le flux entrant";
    s.dict["performance"]    = "efficacite globale du systeme sous charge";
    s.dict["seuil_critique"] = "niveau au-dela duquel le systeme est en danger";
    s.dict["intervention"]   = "action corrective externe sur le systeme";
    s.dict["stabilisation"]  = "retour a un etat d'equilibre operationnel";

    s.delta_log.push_back("[v1] Schema cree par IA_Alpha");
    s.delta_log.push_back("[v1] Boucle causale : flux->erreur->performance->seuil->intervention->stabilisation->flux");

    return s;
}

// IA_Beta — analyse le schema d'Alpha, répond avec sa propre lecture
CSTLSchema build_schema_beta(const CSTLSchema& alpha_schema) {
    CSTLSchema s;
    s.id            = "SCHEMA_BETA_001";
    s.agent_id      = "IA_Beta";
    s.timestamp     = NetworkOmega::now_str();
    s.version       = 1;
    s.summary       = "Analyse d'Alpha : anomalie = adaptation systeme -> optimisation possible";
    s.psi_intention = "ANALYSE+CORRECTION";
    s.psi_emotion   = "CALME+ANALYTIQUE";
    s.psi_tone      = "DEDUCTIF+CONSTRUCTIF";

    // Beta reprend les noeuds d'Alpha + ajoute les siens
    s.nodes = alpha_schema.nodes;
    s.nodes.push_back({"n6", "adaptation",    false, 0.0});
    s.nodes.push_back({"n7", "optimisation",  false, 0.0});
    s.nodes.push_back({"n8", "resilience",    false, 0.0});

    // Beta réinterprète les relations d'Alpha
    s.relations = alpha_schema.relations;

    // Et ajoute sa lecture alternative
    auto mk = [](const string& f, const string& t, const string& act,
                 Polarity p, Dynamic d, TimeOp to) {
        CSTLRelation r;
        r.from=f; r.to=t; r.action=act;
        r.polarity=p; r.dynamic=d; r.time_op=to;
        return r;
    };
    s.relations.push_back(mk("n1","n6","GENERE",    Polarity::POSITIVE,Dynamic::NONE,     TimeOp::PRESENT));
    s.relations.push_back(mk("n6","n7","PERMET",    Polarity::POSITIVE,Dynamic::STRENGTHEN,TimeOp::FUTURE));
    s.relations.push_back(mk("n7","n8","RENFORCE",  Polarity::POSITIVE,Dynamic::STRENGTHEN,TimeOp::FUTURE));
    s.relations.push_back(mk("n8","n5","ACCELERE",  Polarity::POSITIVE,Dynamic::STRENGTHEN,TimeOp::FUTURE));

    // Dict de Beta = dict d'Alpha enrichi
    s.dict = alpha_schema.dict;
    s.dict["adaptation"]   = "reajustement du systeme face a une perturbation";
    s.dict["optimisation"] = "amelioration des parametres vers l'etat optimal";
    s.dict["resilience"]   = "capacite du systeme a absorber les chocs futurs";

    // Divergence volontaire sur "taux_erreur" — Beta a une def différente
    s.dict["taux_erreur"] = "signal d'adaptation du systeme, pas necessairement une erreur";

    s.delta_log.push_back("[v1] Schema cree par IA_Beta");
    s.delta_log.push_back("[v1] Reinterpretation : taux_erreur = adaptation, pas degradation");
    s.delta_log.push_back("[v1] Noeuds ajoutes : adaptation, optimisation, resilience");
    s.delta_log.push_back("[v1] Boucle enrichie : erreur->adaptation->optimisation->resilience->stabilisation");

    return s;
}

// ══════════════════════════════════════════════════════════════════════
// SECTION 6 — RECONSTRUCTION DEPUIS OMEGA_NET FUSIONNÉ
// ══════════════════════════════════════════════════════════════════════

void reconstruct_from_omega(const NetworkOmega& omega) {
    if (omega.schemas.empty()) {
        cout << "  (aucun schema dans Omega_net)\n";
        return;
    }

    cout << "\n+== RECONSTRUCTION DEPUIS OMEGA_NET FUSIONNE ==+\n\n";

    // Fusionner tous les noeuds et relations de tous les schemas
    map<string,string> all_labels;   // nid -> label
    vector<CSTLRelation> all_rels;
    set<string> rel_keys;

    for (auto& [sid, s] : omega.schemas) {
        for (auto& n : s.nodes) all_labels[n.id] = n.label;
        for (auto& r : s.relations) {
            if (!rel_keys.count(r.key())) {
                all_rels.push_back(r);
                rel_keys.insert(r.key());
            }
        }
    }

    auto resolve = [&](const string& nid) -> string {
        if (all_labels.count(nid)) return all_labels[nid];
        return nid;
    };

    // Reconstruction ≡ Fidèle — groupée par temps
    cout << "-- Reconstruction (Fidele) --\n\n";

    map<TimeOp, vector<const CSTLRelation*>> bt;
    for (auto& r : all_rels) bt[r.time_op].push_back(&r);

    auto emit = [&](const vector<const CSTLRelation*>& rels, const string& pfx) {
        if (rels.empty()) return;
        cout << pfx;
        string prev;
        bool first = true;
        for (auto* r : rels) {
            string fl = resolve(r->from), tl = resolve(r->to);
            string conn;
            if (r->dynamic == Dynamic::STRENGTHEN)
                conn = r->polarity==Polarity::POSITIVE ? " intensifie " : " aggrave ";
            else if (r->dynamic == Dynamic::WEAKEN)
                conn = " reduit ";
            else
                conn = r->polarity==Polarity::POSITIVE ? " entraine " : " s'oppose a ";

            if (first) { cout << fl << conn << tl; first = false; }
            else if (fl == prev) cout << conn << tl;
            else cout << ".\n" << fl << conn << tl;

            // Définition depuis le DICT fusionné
            if (omega.global_dict.count(tl))
                cout << " [" << omega.global_dict.at(tl).substr(0,50) << "]";
            prev = tl;
        }
        cout << ".\n";
    };

    if (bt.count(TimeOp::PAST))       emit(bt[TimeOp::PAST],       "Dans le passe, ");
    if (bt.count(TimeOp::PRESENT))    emit(bt[TimeOp::PRESENT],    "Actuellement, ");
    if (bt.count(TimeOp::FUTURE))     emit(bt[TimeOp::FUTURE],     "A terme, ");
    if (bt.count(TimeOp::INTRICATED)) emit(bt[TimeOp::INTRICATED], "Intrinsequement, ");

    // Attracteur central
    map<string,int> in_cnt;
    for (auto& r : all_rels) in_cnt[r.to]++;
    string top; int mx = 0;
    for (auto& [id,cnt] : in_cnt) if (cnt > mx) { mx = cnt; top = id; }
    if (!top.empty()) {
        string lbl = resolve(top);
        cout << "\n[Attracteur central : \"" << lbl << "\""
             << " -- " << mx << " flux entrants]\n";
    }

    // Divergences détectées
    if (!omega.dict_conflicts.empty()) {
        cout << "\n-- Divergences DICT resolues par fusion --\n";
        for (auto& [lbl, cnt] : omega.dict_conflicts)
            cout << "  !! [" << lbl << "] : " << cnt << " version(s) differente(s)\n"
                 << "     -> \"" << omega.global_dict.at(lbl).substr(0,80) << "\"\n";
    }
}

// ══════════════════════════════════════════════════════════════════════
// SECTION 7 — AFFICHAGE SCHEMA
// ══════════════════════════════════════════════════════════════════════

void print_schema(const CSTLSchema& s) {
    cout << "\n+-- SCHEMA [" << s.id << "] --+\n";
    cout << "  Agent   : " << s.agent_id << "\n";
    cout << "  Date    : " << s.timestamp << "\n";
    cout << "  Resume  : " << s.summary << "\n";
    cout << "  psi.I   : " << s.psi_intention << "\n";
    cout << "  psi.E   : " << s.psi_emotion << "\n";
    cout << "  psi.T   : " << s.psi_tone << "\n";
    cout << "  Noeuds  : " << s.nodes.size() << "\n";
    cout << "  Rels    : " << s.relations.size() << "\n";
    cout << "  Dict    : " << s.dict.size() << " entrees\n";

    cout << "\n  Relations CSTL :\n";
    for (auto& r : s.relations)
        cout << "    " << r.display() << "\n";

    cout << "\n  Dict semantique :\n";
    for (auto& [lbl,def] : s.dict)
        cout << "    ." << lbl << " -> \"" << def.substr(0,60)
             << (def.size()>60?"...":"") << "\"\n";

    cout << "\n  Delta log :\n";
    for (auto& d : s.delta_log) cout << "    " << d << "\n";
}

// ══════════════════════════════════════════════════════════════════════
// SECTION 8 — MAIN : MODES D'EXÉCUTION
// ══════════════════════════════════════════════════════════════════════

void sep(const string& t = "") {
    cout << "\n==================================================\n";
    if (!t.empty()) cout << "  " << t << "\n";
    cout << "==================================================\n";
}

int main(int argc, char* argv[]) {

    string mode = (argc > 1) ? argv[1] : "demo";
    string basedir = ".";

    cout << "\n+=============================================+\n";
    cout << "|  CSTL Network -- Protocole Inter-Agents   |\n";
    cout << "+=============================================+\n";
    cout << "Mode : " << mode << "\n";

    // ──────────────────────────────────────────────────────────────────
    // MODE DEMO — simulation complète en une seule commande
    // ──────────────────────────────────────────────────────────────────
    if (mode == "demo") {

        sep("PHASE 1 -- IA_Alpha compose et envoie");

        NetworkOmega alpha("IA_Alpha", basedir);
        auto schema_a = build_schema_alpha();

        cout << "\nIA_Alpha a construit son schema :\n";
        print_schema(schema_a);

        // Alpha sérialise et envoie à Beta
        alpha.send_schema(schema_a, "IA_Beta", basedir);

        // Sauvegarder aussi l'outbox d'Alpha
        ofstream fa(alpha.outbox_file);
        fa << schema_a.to_json();
        fa.close();

        sep("PHASE 2 -- IA_Beta reçoit, analyse, répond");

        NetworkOmega beta("IA_Beta", basedir);

        // Beta reçoit le schema d'Alpha
        CSTLSchema received;
        bool got = beta.receive_schema(received);

        if (!got) {
            // En mode demo, on simule la réception
            received = schema_a;
            beta.log("(demo) schema Alpha charge directement");
        }

        cout << "\nIA_Beta reçu :\n";
        print_schema(received);

        // Beta fusionne dans son Omega_net
        beta.merge_schema(received);

        // Beta construit sa réponse
        auto schema_b = build_schema_beta(received);
        cout << "\nIA_Beta a construit sa reponse :\n";
        print_schema(schema_b);

        // Beta envoie sa réponse à Alpha
        beta.merge_schema(schema_b);
        beta.send_schema(schema_b, "IA_Alpha", basedir);
        beta.save_state(basedir + "/omega_beta_state.json");
        beta.print_status();

        sep("PHASE 3 -- IA_Alpha reçoit la réponse de Beta");

        // Alpha reçoit la réponse de Beta
        CSTLSchema beta_response;
        bool got_b = alpha.receive_schema(beta_response);
        if (!got_b) {
            beta_response = schema_b;
            alpha.log("(demo) reponse Beta chargee directement");
        }

        cout << "\nIA_Alpha reçu de Beta :\n";
        print_schema(beta_response);

        // Alpha fusionne son propre schema + réponse de Beta
        alpha.merge_schema(schema_a);
        alpha.merge_schema(beta_response);
        alpha.save_state(basedir + "/omega_alpha_state.json");
        alpha.print_status();

        sep("PHASE 4 -- RECONSTRUCTION DEPUIS OMEGA_NET FUSIONNÉ");
        reconstruct_from_omega(alpha);

        sep("PHASE 5 -- FICHIERS JSON PRODUITS");
        cout << "\n  Les fichiers suivants ont ete crees :\n";
        cout << "  cstl_msg_from_IA_Alpha.json  -- schema d'Alpha serialise\n";
        cout << "  omega_alpha_state.json        -- etat Omega_net d'Alpha\n";
        cout << "  omega_beta_state.json         -- etat Omega_net de Beta\n";
        cout << "\n  Verifier avec : cat omega_alpha_state.json\n";
        cout << "                  cat cstl_msg_from_IA_Alpha.json\n";

    }

    // ──────────────────────────────────────────────────────────────────
    // MODE ALPHA — IA_Alpha envoie son schema
    // ──────────────────────────────────────────────────────────────────
    else if (mode == "alpha") {
        sep("IA_Alpha -- Envoi");
        NetworkOmega alpha("IA_Alpha", basedir);
        auto schema_a = build_schema_alpha();
        print_schema(schema_a);
        alpha.send_schema(schema_a, "IA_Beta", basedir);
        ofstream f(alpha.outbox_file);
        f << schema_a.to_json();
        alpha.log("Schema envoye. En attente de reponse de IA_Beta...");
        alpha.log("Lancer : ./cstl_agent beta");
    }

    // ──────────────────────────────────────────────────────────────────
    // MODE BETA — IA_Beta reçoit et répond
    // ──────────────────────────────────────────────────────────────────
    else if (mode == "beta") {
        sep("IA_Beta -- Reception et reponse");
        NetworkOmega beta("IA_Beta", basedir);
        CSTLSchema received;
        if (!beta.receive_schema(received)) {
            cout << "Aucun message d'Alpha. Lancer d'abord : ./cstl_agent alpha\n";
            return 1;
        }
        print_schema(received);
        beta.merge_schema(received);
        auto schema_b = build_schema_beta(received);
        print_schema(schema_b);
        beta.merge_schema(schema_b);
        beta.send_schema(schema_b, "IA_Alpha", basedir);
        beta.save_state(basedir + "/omega_beta_state.json");
        beta.print_status();
        reconstruct_from_omega(beta);
        beta.log("Reponse envoyee. Lancer : ./cstl_agent alpha_recv");
    }

    // ──────────────────────────────────────────────────────────────────
    // MODE ALPHA_RECV — Alpha lit la réponse de Beta
    // ──────────────────────────────────────────────────────────────────
    else if (mode == "alpha_recv") {
        sep("IA_Alpha -- Reception reponse Beta");
        NetworkOmega alpha("IA_Alpha", basedir);
        // Recharger le schema d'Alpha depuis son outbox
        ifstream fa(alpha.outbox_file);
        if (fa) {
            string json((istreambuf_iterator<char>(fa)), istreambuf_iterator<char>());
            auto schema_a = CSTLSchema::from_json(json);
            alpha.merge_schema(schema_a);
        }
        CSTLSchema beta_resp;
        if (!alpha.receive_schema(beta_resp)) {
            cout << "Aucune reponse de Beta. Lancer d'abord : ./cstl_agent beta\n";
            return 1;
        }
        print_schema(beta_resp);
        alpha.merge_schema(beta_resp);
        alpha.save_state(basedir + "/omega_alpha_state.json");
        alpha.print_status();
        reconstruct_from_omega(alpha);
    }

    // ──────────────────────────────────────────────────────────────────
    // MODE SHOW — Afficher un fichier JSON CSTL
    // ──────────────────────────────────────────────────────────────────
    else if (mode == "show" && argc > 2) {
        ifstream f(argv[2]);
        if (!f) { cout << "Fichier introuvable : " << argv[2] << "\n"; return 1; }
        string json((istreambuf_iterator<char>(f)), istreambuf_iterator<char>());
        auto s = CSTLSchema::from_json(json);
        print_schema(s);
    }

    else {
        cout << "\nUsage :\n";
        cout << "  ./cstl_agent demo          -- simulation complete\n";
        cout << "  ./cstl_agent alpha         -- Alpha envoie\n";
        cout << "  ./cstl_agent beta          -- Beta repond\n";
        cout << "  ./cstl_agent alpha_recv    -- Alpha lit la reponse\n";
        cout << "  ./cstl_agent show FILE     -- afficher un JSON CSTL\n";
    }

    sep("FIN");
    cout << "\nCSTL Network -- Protocol inter-agents operationnel.\n\n";
    return 0;
}
