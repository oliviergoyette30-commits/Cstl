// ╔══════════════════════════════════════════════════════════╗
// ║  CSTL Parser v2 — Parseur Naturel Amélioré             ║
// ║  UTF-8 propre · Multi-sujets · Polarité · Temps        ║
// ║  Densité · Déduplication · Graphe enrichi              ║
// ╚══════════════════════════════════════════════════════════╝
//
// COMPILATION : g++ -std=c++17 -O2 -o cstl_parser cstl_parser_v2.cpp
// USAGE       : ./cstl_parser
//               (entrer du texte, "END" pour terminer)

#include <algorithm>
#include <iostream>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <unordered_map>
#include <vector>

using namespace std;

// ══════════════════════════════════════════════════════════════
// SECTION 1 — RELATION CSTL
// ══════════════════════════════════════════════════════════════

enum class Polarity  { POSITIVE, NEGATIVE, NEUTRAL };
enum class TimeOp    { PAST, PRESENT, FUTURE };
enum class Dynamic   { STRENGTHEN, WEAKEN, NONE };

struct CSTLRelation {
    string   from, to;
    string   action;      // verbe normalisé
    Polarity polarity   = Polarity::POSITIVE;
    TimeOp   time_op    = TimeOp::PRESENT;
    Dynamic  dynamic    = Dynamic::NONE;
    int      count      = 1;  // pour pondération par fréquence

    // Clé unique pour déduplication
    string key() const {
        return from + "|" + action + "|" + to;
    }
};

// ══════════════════════════════════════════════════════════════
// SECTION 2 — NORMALISATION UTF-8
// ══════════════════════════════════════════════════════════════

// Remplacement UTF-8 propre — octet par octet
string replace_utf8(const string& src,
                    const string& from_utf8,
                    const string& to_ascii) {
    string result;
    size_t i = 0;
    while (i < src.size()) {
        if (i + from_utf8.size() <= src.size() &&
            src.substr(i, from_utf8.size()) == from_utf8) {
            result += to_ascii;
            i += from_utf8.size();
        } else {
            result += src[i++];
        }
    }
    return result;
}

// Table complète des accentués français → ASCII
string normalize_accents(string w) {
    // é è ê ë → e (UTF-8 2 bytes chacun)
    w = replace_utf8(w, "\xC3\xA9", "e"); // é
    w = replace_utf8(w, "\xC3\xA8", "e"); // è
    w = replace_utf8(w, "\xC3\xAA", "e"); // ê
    w = replace_utf8(w, "\xC3\xAB", "e"); // ë
    // à â → a
    w = replace_utf8(w, "\xC3\xA0", "a"); // à
    w = replace_utf8(w, "\xC3\xA2", "a"); // â
    // ù û → u
    w = replace_utf8(w, "\xC3\xB9", "u"); // ù
    w = replace_utf8(w, "\xC3\xBB", "u"); // û
    // î ï → i
    w = replace_utf8(w, "\xC3\xAE", "i"); // î
    w = replace_utf8(w, "\xC3\xAF", "i"); // ï
    // ô → o
    w = replace_utf8(w, "\xC3\xB4", "o"); // ô
    // ç → c
    w = replace_utf8(w, "\xC3\xA7", "c"); // ç
    // œ → oe
    w = replace_utf8(w, "\xC5\x93", "oe"); // œ
    // Majuscules accentuées
    w = replace_utf8(w, "\xC3\x89", "e"); // É
    w = replace_utf8(w, "\xC3\x80", "a"); // À
    w = replace_utf8(w, "\xC3\x87", "c"); // Ç
    return w;
}

// Normalisation complète d'un mot
string normalize(string w) {
    // 1. Minuscules ASCII
    for (char& c : w)
        if (c >= 'A' && c <= 'Z') c = c - 'A' + 'a';

    // 2. Accents → ASCII
    w = normalize_accents(w);

    // 3. Désinflexions — ordre : du plus spécifique au plus général

    // Verbes en -issent → -ir
    if (w.size() > 7 && w.substr(w.size()-6) == "issent")
        return w.substr(0, w.size()-6) + "ir";
    // Verbes en -issons → -ir
    if (w.size() > 7 && w.substr(w.size()-6) == "issons")
        return w.substr(0, w.size()-6) + "ir";

    // Verbes 3e pers. pluriel en -ent
    // Ne pas toucher : -ment (adverbes/noms), -ient (3pp de venir/tenir)
    if (w.size() > 5) {
        string suf4 = w.substr(w.size()-4);
        string suf5 = (w.size()>5) ? w.substr(w.size()-5) : "";
        bool is_verb_ent = (w.substr(w.size()-3) == "ent")
                        && suf4 != "ient"     // revient, vient
                        && suf5 != "aient"    // auraient
                        && suf4 != "ment"     // absolument
                        && suf5 != "iment";   // notamment
        if (is_verb_ent)
            w = w.substr(0, w.size()-3) + "e";
    }
    // Verbes en -ons
    if (w.size() > 5 && w.substr(w.size()-3) == "ons"
        && w.substr(w.size()-4) != "ions")
        w = w.substr(0, w.size()-3) + "er";
    // Verbes en -ez
    if (w.size() > 4 && w.substr(w.size()-2) == "ez")
        w = w.substr(0, w.size()-2) + "er";

    // Pluriels nominaux — ne pas désinflexir -ions, -tions, -sions
    if (w.size() > 4 && w.back() == 's') {
        string suf4 = w.substr(w.size()-4);
        if (suf4 != "ions" && suf4 != "tion" &&
            w.substr(w.size()-2) != "is" &&
            w.substr(w.size()-2) != "as" &&
            w.substr(w.size()-2) != "os" &&
            w.substr(w.size()-2) != "ss")   // "stress" → garder
            w.pop_back();
    }

    return w;
}

// ══════════════════════════════════════════════════════════════
// SECTION 3 — TOKENISATION
// ══════════════════════════════════════════════════════════════

struct Token {
    string raw;       // mot original
    string norm;      // mot normalisé
    int    pos = 0;   // position dans la phrase
};

vector<Token> tokenize(const string& text) {
    vector<Token> tokens;
    string current;
    int pos = 0;

    // Séparer sur tout ce qui n'est pas alphanumérique ou UTF-8 multi-octet
    size_t i = 0;
    while (i < text.size()) {
        unsigned char c = (unsigned char)text[i];

        if (c >= 128) {
            // Caractère UTF-8 multi-octet — on l'inclut
            current += text[i++];
            // Continuer à lire les bytes de continuation
            while (i < text.size() && ((unsigned char)text[i] & 0xC0) == 0x80)
                current += text[i++];
        } else if (isalnum(c) || c == '\'') {
            current += (char)tolower(c);
            ++i;
        } else {
            if (!current.empty()) {
                // Gérer l'élision (l'→ l + suite, d'→ d + suite)
                size_t ap = current.find('\'');
                if (ap != string::npos) {
                    // Garder la partie après l'apostrophe comme token
                    string after = current.substr(ap + 1);
                    if (!after.empty()) {
                        Token t;
                        t.raw  = after;
                        t.norm = normalize(after);
                        t.pos  = pos++;
                        tokens.push_back(t);
                    }
                } else {
                    Token t;
                    t.raw  = current;
                    t.norm = normalize(current);
                    t.pos  = pos++;
                    tokens.push_back(t);
                }
                current.clear();
            }
            ++i;
        }
    }
    if (!current.empty()) {
        Token t; t.raw = current; t.norm = normalize(current); t.pos = pos;
        tokens.push_back(t);
    }
    return tokens;
}

// ══════════════════════════════════════════════════════════════
// SECTION 4 — DICTIONNAIRES SÉMANTIQUES
// ══════════════════════════════════════════════════════════════

struct ActionDef {
    string   cstl_action;   // nom CSTL normalisé
    Polarity polarity;      // POSITIVE / NEGATIVE / NEUTRAL
    Dynamic  dynamic;       // STRENGTHEN / WEAKEN / NONE
};

// Table des verbes d'action → définition CSTL
const unordered_map<string, ActionDef> ACTION_MAP = {
    // ── Positifs ─────────────────────────────────────────────
    {"analyse",      {"ANALYSE",      Polarity::POSITIVE, Dynamic::NONE}},
    {"analysent",    {"ANALYSE",      Polarity::POSITIVE, Dynamic::NONE}},
    {"ameliore",     {"AMELIORE",     Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"ameliorent",   {"AMELIORE",     Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"ameliorer",    {"AMELIORE",     Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"produit",      {"PRODUIT",      Polarity::POSITIVE, Dynamic::NONE}},
    {"produisent",   {"PRODUIT",      Polarity::POSITIVE, Dynamic::NONE}},
    {"produise",     {"PRODUIT",      Polarity::POSITIVE, Dynamic::NONE}},
    {"apprend",      {"APPREND",      Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"apprennent",   {"APPREND",      Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"transforme",   {"TRANSFORME",   Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"transforment", {"TRANSFORME",   Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"renforce",     {"RENFORCE",     Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"renforcent",   {"RENFORCE",     Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"genere",       {"GENERE",       Polarity::POSITIVE, Dynamic::NONE}},
    {"generent",     {"GENERE",       Polarity::POSITIVE, Dynamic::NONE}},
    {"cree",         {"CREE",         Polarity::POSITIVE, Dynamic::NONE}},
    {"creent",       {"CREE",         Polarity::POSITIVE, Dynamic::NONE}},
    {"cause",        {"CAUSE",        Polarity::POSITIVE, Dynamic::NONE}},
    {"causent",      {"CAUSE",        Polarity::POSITIVE, Dynamic::NONE}},
    {"entraine",     {"ENTRAINE",     Polarity::POSITIVE, Dynamic::NONE}},
    {"entrainent",   {"ENTRAINE",     Polarity::POSITIVE, Dynamic::NONE}},
    {"favorise",     {"FAVORISE",     Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"favorisent",   {"FAVORISE",     Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"stimule",      {"STIMULE",      Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"stimulent",    {"STIMULE",      Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"accelere",     {"ACCELERE",     Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"accelerent",   {"ACCELERE",     Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"developpe",    {"DEVELOPPE",    Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"developpent",  {"DEVELOPPE",    Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"augmente",     {"AUGMENTE",     Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"augmentent",   {"AUGMENTE",     Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"permet",       {"PERMET",       Polarity::POSITIVE, Dynamic::NONE}},
    {"permettent",   {"PERMET",       Polarity::POSITIVE, Dynamic::NONE}},
    {"utilise",      {"UTILISE",      Polarity::POSITIVE, Dynamic::NONE}},
    {"utilisent",    {"UTILISE",      Polarity::POSITIVE, Dynamic::NONE}},
    {"active",       {"ACTIVE",       Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"activent",     {"ACTIVE",       Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"optimise",     {"OPTIMISE",     Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    {"optimisent",   {"OPTIMISE",     Polarity::POSITIVE, Dynamic::STRENGTHEN}},
    // ── Négatifs ─────────────────────────────────────────────
    {"deteriore",    {"DETERIORE",    Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"deteriorent",  {"DETERIORE",    Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"reduit",       {"REDUIT",       Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"reduisent",    {"REDUIT",       Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"diminue",      {"DIMINUE",      Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"diminuent",    {"DIMINUE",      Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"detruit",      {"DETRUIT",      Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"detruisent",   {"DETRUIT",      Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"menace",       {"MENACE",       Polarity::NEGATIVE, Dynamic::NONE}},
    {"menacent",     {"MENACE",       Polarity::NEGATIVE, Dynamic::NONE}},
    {"degrade",      {"DEGRADE",      Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"degradent",    {"DEGRADE",      Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"affaiblit",    {"AFFAIBLIT",    Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"affaiblissent",{"AFFAIBLIT",    Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"bloque",       {"BLOQUE",       Polarity::NEGATIVE, Dynamic::NONE}},
    {"bloquent",     {"BLOQUE",       Polarity::NEGATIVE, Dynamic::NONE}},
    {"empeche",      {"EMPECHE",      Polarity::NEGATIVE, Dynamic::NONE}},
    {"empechent",    {"EMPECHE",      Polarity::NEGATIVE, Dynamic::NONE}},
    {"limite",       {"LIMITE",       Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"limitent",     {"LIMITE",       Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"nuit",         {"NUIT",         Polarity::NEGATIVE, Dynamic::WEAKEN}},
    {"nuisent",      {"NUIT",         Polarity::NEGATIVE, Dynamic::WEAKEN}},
    // ── Tension / opposition ──────────────────────────────────
    {"s'oppose",     {"OPPOSE",       Polarity::NEUTRAL,  Dynamic::NONE}},
    {"s'opposent",   {"OPPOSE",       Polarity::NEUTRAL,  Dynamic::NONE}},
    {"contraste",    {"CONTRASTE",    Polarity::NEUTRAL,  Dynamic::NONE}},
    {"concurrence",  {"CONCURRENCE",  Polarity::NEUTRAL,  Dynamic::NONE}},
};

// Mots indicateurs de temps
const set<string> FUTURE_MARKERS = {
    "va", "vont", "sera", "seront", "voudra", "pourrait",
    "permettra", "ira", "devra", "pourra"
};
const set<string> PAST_MARKERS = {
    "avait", "avaient", "etait", "etaient", "fut", "furent", "a"
};

// Mots-bruit (stopwords)
const set<string> NOISE = {
    "le","la","les","un","une","des","du","de","d",
    "et","ou","mais","donc","car","ni","or",
    "dans","sur","sous","avec","sans","pour","par","en",
    "au","aux","ce","cet","cette","ces","mon","ton","son",
    "notre","votre","leur","leurs","il","elle","ils","elles",
    "qui","que","qu","dont","ou","y","en","lui","eux",
    "aussi","tres","plus","moins","bien","tout","tous",
    "je","tu","nous","vous","on","me","te","se"
};

bool is_noise(const string& w) {
    return NOISE.count(w) > 0;
}

// ══════════════════════════════════════════════════════════════
// SECTION 5 — MÉMOIRE DES ENTITÉS
// ══════════════════════════════════════════════════════════════

class EntityMemory {
public:
    vector<string>         history;           // ordre d'apparition
    map<string,int>        freq;              // fréquence → densité
    static const int       MAX_HISTORY = 10;

    void add(const string& e) {
        if (e.empty() || is_noise(e)) return;
        if (history.empty() || history.back() != e)
            history.push_back(e);
        if (history.size() > MAX_HISTORY)
            history.erase(history.begin());
        freq[e]++;
    }

    string last() const {
        if (!history.empty()) return history.back();
        return "";
    }

    // Résoudre les pronoms
    string resolve(const string& w) const {
        if (w == "il" || w == "elle" || w == "ils" || w == "elles"
            || w == "celui" || w == "cela" || w == "ce")
            return last();
        return w;
    }

    // Densité relative (fréquence normalisée)
    double density(const string& e) const {
        if (freq.empty()) return 0.0;
        int max_f = 0;
        for (auto& [k,v] : freq) max_f = max(max_f, v);
        auto it = freq.find(e);
        if (it == freq.end()) return 0.0;
        return (double)it->second / max_f;
    }

    void print_density() const {
        cout << "\n=== DENSITÉ DES ENTITÉS ===\n";
        // Trier par fréquence décroissante
        vector<pair<int,string>> sorted;
        for (auto& [k,v] : freq) sorted.push_back({v, k});
        sort(sorted.rbegin(), sorted.rend());
        for (auto& [f, e] : sorted) {
            double d = density(e);
            cout << "  ∙" << e << " [d=" << fixed;
            cout.precision(2);
            cout << d << ", f=" << f << "]";
            if (d >= 0.8) cout << " ◉ ATTRACTEUR";
            else if (d >= 0.5) cout << " ⚠ DENSE";
            cout << "\n";
        }
    }
};

// ══════════════════════════════════════════════════════════════
// SECTION 6 — GRAPHE CSTL GLOBAL
// ══════════════════════════════════════════════════════════════

class CSTLGraph {
public:
    map<string, vector<CSTLRelation>> adjacency;  // from → relations

    // Ajouter ou incrémenter une relation (déduplication)
    void add(const CSTLRelation& rel) {
        auto& rels = adjacency[rel.from];
        for (auto& r : rels) {
            if (r.key() == rel.key()) {
                r.count++;
                return;
            }
        }
        rels.push_back(rel);
    }

    // Détecter les boucles causales (DFS simple)
    vector<vector<string>> detect_cycles() const {
        vector<vector<string>> cycles;
        set<string> visited;
        for (auto& [start, _] : adjacency) {
            vector<string> path = {start};
            set<string> in_path = {start};
            dfs_cycle(start, start, path, in_path, cycles);
        }
        return cycles;
    }

    void print() const {
        cout << "\n=== CSTL GLOBAL GRAPH ===\n";
        for (auto& [from, rels] : adjacency) {
            cout << "∙" << from << " {\n";
            for (auto& r : rels) {
                // Symbole de polarité
                string pol = (r.polarity == Polarity::POSITIVE) ? "⁺" :
                             (r.polarity == Polarity::NEGATIVE) ? "⁻" : "°";
                // Symbole de dynamique
                string dyn = (r.dynamic == Dynamic::STRENGTHEN) ? "↑" :
                             (r.dynamic == Dynamic::WEAKEN)     ? "↓" : "";
                // Symbole de temps
                string time = (r.time_op == TimeOp::PAST)   ? "«" :
                              (r.time_op == TimeOp::FUTURE)  ? "»" : "=";
                cout << "  " << time << " →" << pol << dyn
                     << " " << r.action << " ∙" << r.to;
                if (r.count > 1)
                    cout << " [×" << r.count << "]";
                cout << "\n";
            }
            cout << "}\n\n";
        }

        // Boucles causales
        auto cycles = detect_cycles();
        if (!cycles.empty()) {
            cout << "=== BOUCLES CAUSALES ↺ ===\n";
            for (auto& c : cycles) {
                for (size_t i = 0; i < c.size(); ++i) {
                    cout << c[i];
                    if (i + 1 < c.size()) cout << " → ";
                }
                cout << " → " << c[0] << " ↺\n";
            }
            cout << "\n";
        }
    }

private:
    void dfs_cycle(const string& start, const string& cur,
                   vector<string>& path, set<string>& in_path,
                   vector<vector<string>>& cycles) const {
        auto it = adjacency.find(cur);
        if (it == adjacency.end()) return;
        for (auto& r : it->second) {
            if (r.to == start && path.size() > 1) {
                cycles.push_back(path);
                return;
            }
            if (in_path.count(r.to) || path.size() > 8) continue;
            path.push_back(r.to);
            in_path.insert(r.to);
            dfs_cycle(start, r.to, path, in_path, cycles);
            path.pop_back();
            in_path.erase(r.to);
        }
    }
};

// ══════════════════════════════════════════════════════════════
// SECTION 7 — CONSTRUCTEUR CSTL AMÉLIORÉ
// ══════════════════════════════════════════════════════════════

class CSTLBuilder {
public:
    EntityMemory memory;
    CSTLGraph    graph;

    // Construire les relations CSTL depuis les tokens d'une phrase
    void build(const vector<Token>& tokens, TimeOp global_time = TimeOp::PRESENT) {
        // Détecter si la phrase contient des marqueurs de temps globaux
        TimeOp detected_time = global_time;
        for (auto& t : tokens) {
            if (FUTURE_MARKERS.count(t.norm)) { detected_time = TimeOp::FUTURE; break; }
            if (PAST_MARKERS.count(t.norm))   { detected_time = TimeOp::PAST;   break; }
        }

        string active_subject = memory.last();

        for (int i = 0; i < (int)tokens.size(); ++i) {
            const string& w = tokens[i].norm;

            // Chercher une action
            auto act_it = ACTION_MAP.find(w);
            if (act_it == ACTION_MAP.end()) continue;

            const ActionDef& adef = act_it->second;

            // ── Trouver le(s) sujet(s) A ────────────────────────────

            // Multi-sujets : "X et Y analysent" → (X,Y) → verbe
            vector<string> subjects;
            bool preceded_by_and = (i > 0 && tokens[i-1].norm == "et");

            if (preceded_by_and && !active_subject.empty()) {
                // Chercher le sujet avant "et"
                for (int j = i - 2; j >= 0; --j) {
                    if (!is_noise(tokens[j].norm)) {
                        subjects.push_back(memory.resolve(tokens[j].norm));
                        break;
                    }
                }
                subjects.push_back(active_subject);
            } else {
                // Sujet le plus proche avant le verbe
                for (int j = i - 1; j >= 0; --j) {
                    const string& cand = tokens[j].norm;
                    if (!is_noise(cand) && ACTION_MAP.find(cand) == ACTION_MAP.end()) {
                        string resolved = memory.resolve(cand);
                        if (!resolved.empty())
                            subjects.push_back(resolved);
                        break;
                    }
                }
                if (subjects.empty() && !active_subject.empty())
                    subjects.push_back(active_subject);
            }

            // ── Trouver l'objet B ────────────────────────────────────
            string B;
            // Sauter les mots-bruit et les auxiliaires après le verbe
            for (int j = i + 1; j < (int)tokens.size(); ++j) {
                const string& cand = tokens[j].norm;
                if (!is_noise(cand) && ACTION_MAP.find(cand) == ACTION_MAP.end()) {
                    B = memory.resolve(cand);
                    break;
                }
            }

            if (B.empty() || subjects.empty()) continue;

            // ── Créer les relations CSTL ─────────────────────────────
            for (auto& A : subjects) {
                if (A.empty() || A == B) continue;

                CSTLRelation rel;
                rel.from     = A;
                rel.to       = B;
                rel.action   = adef.cstl_action;
                rel.polarity = adef.polarity;
                rel.dynamic  = adef.dynamic;
                rel.time_op  = detected_time;

                // Affichage CSTL
                string pol  = (rel.polarity == Polarity::POSITIVE) ? "⁺" :
                              (rel.polarity == Polarity::NEGATIVE) ? "⁻" : "°";
                string dyn  = (rel.dynamic  == Dynamic::STRENGTHEN) ? "↑" :
                              (rel.dynamic  == Dynamic::WEAKEN)     ? "↓" : "";
                string time = (rel.time_op  == TimeOp::PAST)   ? "«" :
                              (rel.time_op  == TimeOp::FUTURE)  ? "»" : "=";

                cout << time << " ∙" << A << " →" << pol << dyn
                     << " " << rel.action << " ∙" << B << "\n";

                graph.add(rel);
                memory.add(A);
            }

            active_subject = subjects.back();
            memory.add(B); // B peut devenir sujet de la prochaine relation
        }
    }
};

// ══════════════════════════════════════════════════════════════
// SECTION 8 — DÉCOUPAGE EN PHRASES
// ══════════════════════════════════════════════════════════════

vector<string> split_sentences(const string& text) {
    vector<string> sentences;
    string current;
    for (char c : text) {
        if (c == '.' || c == '!' || c == '?' || c == ';') {
            if (!current.empty()) {
                sentences.push_back(current);
                current.clear();
            }
        } else {
            current += c;
        }
    }
    if (!current.empty())
        sentences.push_back(current);
    return sentences;
}

// ══════════════════════════════════════════════════════════════
// MAIN
// ══════════════════════════════════════════════════════════════

int main() {
    cout << "\n╔══════════════════════════════════════╗\n";
    cout << "║  CSTL Parser v2 — Parseur Naturel   ║\n";
    cout << "║  Entrer du texte, END pour terminer  ║\n";
    cout << "╚══════════════════════════════════════╝\n\n";
    cout << "Texte (END pour finir) :\n";

    string input, line;
    while (true) {
        getline(cin, line);
        if (line == "END") break;
        input += line + " ";
    }

    if (input.empty()) {
        cout << "Aucun texte fourni.\n";
        return 0;
    }

    CSTLBuilder builder;
    auto sentences = split_sentences(input);
    int id = 1;

    for (auto& s : sentences) {
        if (s.find_first_not_of(" \t\n\r") == string::npos) continue;
        cout << "\n─── Phrase " << id++ << " ───\n";
        auto tokens = tokenize(s);

        // Afficher les tokens normalisés
        cout << "Tokens : ";
        for (auto& t : tokens) cout << "[" << t.norm << "] ";
        cout << "\n";

        cout << "CSTL   :\n";
        builder.build(tokens);
    }

    // Affichage du graphe global
    builder.graph.print();

    // Densité des entités
    builder.memory.print_density();

    cout << "\n─── Mémoire ─── ";
    for (auto& e : builder.memory.history)
        cout << e << " → ";
    cout << "(fin)\n\n";

    return 0;
}
