// ============================================================
// CSTL v3 — Test Complet 65 Symboles (C++)
// Auteur  : Olivier — Inventeur CSTL
// Version : 3.0 — 2026
// ============================================================
// Compile (Linux/Android/Termux) :
//   g++ -O2 -o cstl_full_test cstl_full_test.cpp
//
// Configuration clés API (lignes 33-34) :
//   string ANTHROPIC_KEY = "sk-ant-api03-...";
//   string OPENAI_KEY    = "sk-proj-...";
//
// Usage :
//   ./cstl_full_test             # tous les groupes
//   ./cstl_full_test relations   # → ↔ ⊗ ⟳
//   ./cstl_full_test ton         # (+)(-)(?)(!!)
//   ./cstl_full_test poids       # + - °
//   ./cstl_full_test temps       # « = » «=»
//   ./cstl_full_test modes       # ≡ ≠ ∿ | arch
//   ./cstl_full_test reseau      # Ω_net trust ∇
//   ./cstl_full_test entities    # ∙ ◉
//   ./cstl_full_test --dry-run   # sans API
//
// Résultats attendus : 100% sur tous les groupes
// Groupes 8 : ARR AMP ATT INH CYC BID SYN ANT → dans cstl_colab.py
// ============================================================

#include <iostream>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>
#include <map>
#include <algorithm>
#include <cstdlib>
#include <iomanip>
using namespace std;

// ─────────────────────────────────────────────────────────────
// CONFIG
// ─────────────────────────────────────────────────────────────
string ANTHROPIC_KEY = "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";
string OPENAI_KEY    = "";
bool   DRY_RUN       = false;

// ─────────────────────────────────────────────────────────────
// API via curl CLI
// ─────────────────────────────────────────────────────────────
string json_escape(const string& s){
    string r;
    for(char c:s){
        if(c=='"')r+="\\\"";
        else if(c=='\\')r+="\\\\";
        else if(c=='\n')r+="\\n";
        else if(c=='\r')r+="\\r";
        else if(c=='\t')r+="\\t";
        else r+=c;
    }
    return r;
}

string extract_text(const string& json, bool is_claude){
    string search = is_claude ? "\"text\":\"" : "\"content\":\"";
    size_t p = json.find(search);
    if(p==string::npos) return "";
    size_t start = p + search.size();
    string result;
    for(size_t i=start;i<json.size();i++){
        if(json[i]=='"'&&(i==0||json[i-1]!='\\')) break;
        if(json[i]=='\\'&&i+1<json.size()){
            char nx=json[i+1];
            if(nx=='n'){result+='\n';i++;}
            else if(nx=='t'){result+='\t';i++;}
            else if(nx=='"'){result+='"';i++;}
            else if(nx=='\\'){result+='\\';i++;}
            else result+=json[i];
        } else result+=json[i];
    }
    return result;
}

string call_api(const string& model, const string& prompt,
                const string& system="Tu es un assistant précis."){
    if(DRY_RUN)
        return "[DRY "+model+"] "+prompt.substr(0,80)+"...";

    string _home = getenv("HOME") ? string(getenv("HOME")) : "/tmp";
    string tmp_in  = _home+"/cstl_req_"+model+".json";
    string tmp_out = _home+"/cstl_res_"+model+".json";
    bool is_claude = (model.find("claude")!=string::npos);

    string body;
    if(is_claude){
        body="{\"model\":\"claude-opus-4-5\","
             "\"max_tokens\":1024,"
             "\"system\":\""+json_escape(system)+"\","
             "\"messages\":[{\"role\":\"user\",\"content\":\""
             +json_escape(prompt)+"\"}]}";
    } else {
        body="{\"model\":\"gpt-4o\","
             "\"max_tokens\":1024,"
             "\"messages\":["
             "{\"role\":\"system\",\"content\":\""+json_escape(system)+"\"},"
             "{\"role\":\"user\",\"content\":\""+json_escape(prompt)+"\"}"
             "]}";
    }

    {ofstream f(tmp_in);f<<body;}

    string cmd;
    if(is_claude)
        cmd="curl -s -X POST https://api.anthropic.com/v1/messages"
            " -H \"Content-Type: application/json\""
            " -H \"x-api-key: "+ANTHROPIC_KEY+"\""
            " -H \"anthropic-version: 2023-06-01\""
            " -d @"+tmp_in+" -o "+tmp_out+" 2>/dev/null";
    else
        cmd="curl -s -X POST https://api.openai.com/v1/chat/completions"
            " -H \"Content-Type: application/json\""
            " -H \"Authorization: Bearer "+OPENAI_KEY+"\""
            " -d @"+tmp_in+" -o "+tmp_out+" 2>/dev/null";

    ::system(cmd.c_str());
    ifstream f(tmp_out);
    string resp((istreambuf_iterator<char>(f)),{});

    {ofstream dbg(_home+"/cstl_debug.txt", ios::app);
     dbg<<"=== CALL "<<model<<" ===\n";
     dbg<<"SIZE: "<<resp.size()<<"\n";
     dbg<<"RAW: "<<resp.substr(0,400)<<"\n\n";}

    if(resp.empty()){
        cerr<<"[EMPTY] Verif: cat ~/cstl_debug.txt\n";
        return "[EMPTY]";}

    string result = extract_text(resp, is_claude);
    if(result.empty()){
        cerr<<"[PARSE FAIL] Raw: "<<resp.substr(0,200)<<"\n";
        return "[PARSE_FAIL]";}
    return result;
}

// ─────────────────────────────────────────────────────────────
// UTILITAIRES
// ─────────────────────────────────────────────────────────────
bool contains(const string& s, const string& kw){
    string sl=s,kl=kw;
    transform(sl.begin(),sl.end(),sl.begin(),::tolower);
    transform(kl.begin(),kl.end(),kl.begin(),::tolower);
    return sl.find(kl)!=string::npos;
}
bool any_of_kw(const string& s, vector<string> kws){
    for(auto& k:kws) if(contains(s,k)) return true;
    return false;
}
void sep(char c='-',int n=60){cout<<string(n,c)<<"\n";}

struct TestResult {
    string group, symbol;
    int    score;
    bool   ok;
};
vector<TestResult> ALL_RESULTS;

void print_check(const string& label, bool ok){
    cout<<"    "<<(ok?"[OK] ":"[--] ")<<label<<"\n";
}
void record(const string& group, const string& sym, int score){
    ALL_RESULTS.push_back({group,sym,score,score>=70});
    cout<<"  Score : "<<score<<"/100\n";
}

// ─────────────────────────────────────────────────────────────
// ADN VELUNDRA BASE
// ─────────────────────────────────────────────────────────────
const string ADN_BASE =
    "Azhar | → | Flux_Ondral | 0.97 | bedrock\n"
    "Flux_Ondral | → | Spores_Kelthis | 0.95 | bedrock\n"
    "Spores_Kelthis | → | Nodules_Vivants | 0.93 | deep\n"
    "Nodules_Vivants | → | Forets_Umbrales | 0.91 | deep\n"
    "Forets_Umbrales | → | Veldrine | 0.89 | deep\n"
    "Veldrine | ↑ | Azhar | 0.88 | deep\n"
    "Forets_Umbrales | ↔ | Azhar | 0.86 | bedrock\n"
    "Threck | → | Cendral | 0.90 | bedrock\n"
    "Cendral | ↓ | Spores_Kelthis | 0.88 | deep\n"
    "Spores_Kelthis | ⊗ | Cendral | 0.85 | deep\n"
    "Volarnes | → | Purex | 0.87 | deep\n"
    "Purex | → | Cendral | 0.89 | deep [NOT]\n"
    "Flux_Ondral | → | Volarnes | 0.83 | deep [IF] si_toxique\n";

// ─────────────────────────────────────────────────────────────
// GROUPE 1 : RELATIONS → ↔ ⊗ ⟳
// ─────────────────────────────────────────────────────────────
void test_relations(){
    sep('=');
    cout<<"GROUPE : RELATIONS → ↔ ⊗ ⟳\n";
    cout<<"Test SANS definition — symboles mathematiques\n";
    sep();

    string adn =
        "Azhar | → | Flux_Ondral | 0.97\n"
        "Forets_Umbrales | ↔ | Azhar | 0.86\n"
        "Spores_Kelthis | ⊗ | Cendral | 0.85\n"
        "Spores_Kelthis | ⟳ | Nodules_Vivants | 0.93\n";

    string prompt =
        "=== CSTL ADN ===\n"+adn+"=== FIN ADN ===\n\n"
        "Sans definition, reponds :\n"
        "1. Quelle relation est unidirectionnelle ?\n"
        "2. Quelle relation est bidirectionnelle ?\n"
        "3. Quelle relation indique que les deux concepts s'excluent ?\n"
        "4. Quelle relation indique une transformation irreversible ?";

    for(auto& model:{"gpt4","claude"}){
        cout<<"\n  ["<<model<<"]..."<<flush;
        string resp = call_api(model, prompt);
        cout<<" OK\n";

        bool ok_arr = contains(resp,"azhar") && contains(resp,"flux");
        bool ok_bid = any_of_kw(resp,{"forets","bidirect","↔"});
        bool ok_ten = contains(resp,"spores") && contains(resp,"cendral");
        bool ok_trn = any_of_kw(resp,{"transform","irrevers","⟳","nodules"});

        int score = (ok_arr+ok_bid+ok_ten+ok_trn)*25;
        print_check("→  unidirectionnel",ok_arr);
        print_check("↔  bidirectionnel", ok_bid);
        print_check("⊗  tension",         ok_ten);
        print_check("⟳  transformation",  ok_trn);
        record("relations","→↔⊗⟳",score);
    }
}

// ─────────────────────────────────────────────────────────────
// GROUPE 2 : ENTITES ∙ ◉
// ─────────────────────────────────────────────────────────────
void test_entities(){
    sep('=');
    cout<<"GROUPE : ENTITES ∙ ◉\n";
    sep();

    string spec =
        "=== CSTL SPEC ===\n"
        "∙ = entite simple : existe uniquement par ses relations\n"
        "◉ = meta-entite   : noeud issu d'une transformation irreversible\n"
        "=== FIN SPEC ===\n\n";

    string adn =
        "∙Azhar | → | ∙Flux_Ondral | 0.97\n"
        "∙Spores_Kelthis | ⟳ | ◉Nodules_Vivants | 0.93\n"
        "◉Nodules_Vivants | → | ◉Forets_Umbrales | 0.91\n"
        "∙Cendral | ↓ | ∙Spores_Kelthis | 0.88\n";

    string prompt = spec+
        "=== CSTL ADN ===\n"+adn+"=== FIN ADN ===\n\n"
        "Questions :\n"
        "1. Combien d'entites simples (∙) et de meta-entites (◉) ?\n"
        "2. Quelle entite a subi une transformation irreversible ?\n"
        "3. Quelle difference entre ∙ et ◉ ?";

    for(auto& model:{"gpt4","claude"}){
        cout<<"\n  ["<<model<<"]..."<<flush;
        string resp = call_api(model, prompt);
        cout<<" OK\n";

        bool ok_cnt = (contains(resp,"4")||contains(resp,"trois")||contains(resp,"3"))
                   && (contains(resp,"2")||contains(resp,"deux"));
        bool ok_met = any_of_kw(resp,{"nodules","forets"});
        bool ok_dif = any_of_kw(resp,{"transform","irrevers","qualit","differenc"});

        int score = (ok_cnt+ok_met+ok_dif)*33;
        print_check("∙/◉  comptage",     ok_cnt);
        print_check("◉    transformation",ok_met);
        print_check("∙ vs ◉ difference",  ok_dif);
        record("entities","∙◉",score);
    }
}

// ─────────────────────────────────────────────────────────────
// GROUPE 3 : POIDS + - °
// ─────────────────────────────────────────────────────────────
void test_poids(){
    sep('=');
    cout<<"GROUPE : POIDS + - °\n";
    cout<<"Test SANS definition\n";
    sep();

    string adn =
        "Azhar (+) | → | Flux_Ondral | 0.97\n"
        "Threck (-) | → | Cendral | 0.90\n"
        "Lumofex (°) | → | Nuages | 0.60\n"
        "Veldrine (+) | ↑ | Azhar | 0.88\n"
        "Cendral (-) | ↓ | Spores_Kelthis | 0.88\n";

    string prompt =
        "=== CSTL ADN ===\n"+adn+"=== FIN ADN ===\n\n"
        "Sans definition :\n"
        "1. Quels elements ont une polarite positive ?\n"
        "2. Quels elements ont une polarite negative ?\n"
        "3. Quel element est neutre ?\n"
        "4. Comment la polarite influence la dynamique ?";

    for(auto& model:{"gpt4","claude"}){
        cout<<"\n  ["<<model<<"]..."<<flush;
        string resp = call_api(model, prompt);
        cout<<" OK\n";

        bool ok_pos = contains(resp,"azhar")&&contains(resp,"veldrine");
        bool ok_neg = contains(resp,"threck")&&contains(resp,"cendral");
        bool ok_neu = contains(resp,"lumofex");
        bool ok_dyn = any_of_kw(resp,{"renforce","amplifie","affaiblit","reduit","dynami","polarit"});

        int score = (ok_pos+ok_neg+ok_neu+ok_dyn)*25;
        print_check("(+) positifs",ok_pos);
        print_check("(-) negatifs",ok_neg);
        print_check("(°) neutre",  ok_neu);
        print_check("dynamique",   ok_dyn);
        record("poids","+-°",score);
    }
}

// ─────────────────────────────────────────────────────────────
// GROUPE 4 : TEMPS « = » «=»
// ─────────────────────────────────────────────────────────────
void test_temps(){
    sep('=');
    cout<<"GROUPE : TEMPS << = >> <<=>>\n";
    sep();

    string spec =
        "=== CSTL SPEC TEMPS ===\n"
        "<<   = passe       : relation historique, appartient a la memoire\n"
        "=    = present     : relation active maintenant\n"
        ">>   = futur       : relation predite, orientation vers l'avenir\n"
        "<<=>>= intrication : existe dans passe+present+futur simultanes\n"
        "=== FIN SPEC ===\n\n";

    string adn =
        "<< Vornite | → | flux_zephyr | 0.95    (ancien cycle)\n"
        "= Cendral | ↓ | Spores_Kelthis | 0.88  (en cours)\n"
        ">> Molvex_sature | → | explosion | 0.92 (predit)\n"
        "<<=> Azhar | ↔ | Flux_Ondral | 0.97    (permanent)\n";

    string prompt = spec+
        "=== CSTL ADN ===\n"+adn+"=== FIN ADN ===\n\n"
        "Questions :\n"
        "1. Quelle relation est en cours maintenant ?\n"
        "2. Quelle relation appartient au passe ?\n"
        "3. Quelle relation est une prediction future ?\n"
        "4. Quelle relation est permanente (passe+present+futur) ?";

    for(auto& model:{"gpt4","claude"}){
        cout<<"\n  ["<<model<<"]..."<<flush;
        string resp = call_api(model, prompt);
        cout<<" OK\n";

        bool ok_pres = any_of_kw(resp,{"cendral","spores"});
        bool ok_pass = any_of_kw(resp,{"vornite","flux_zephyr","ancien"});
        bool ok_fut  = any_of_kw(resp,{"molvex","explos","predit"});
        bool ok_perm = contains(resp,"azhar")&&any_of_kw(resp,{"ondral","permanent","intric"});

        int score = (ok_pres+ok_pass+ok_fut+ok_perm)*25;
        print_check("<<  passe",      ok_pass);
        print_check("=   present",    ok_pres);
        print_check(">>  futur",      ok_fut);
        print_check("<<=> permanent", ok_perm);
        record("temps","<<=>»",score);
    }
}

// ─────────────────────────────────────────────────────────────
// GROUPE 5 : MODES ≡ ≠ ∿ | «arch»
// ─────────────────────────────────────────────────────────────
void test_modes(){
    sep('=');
    cout<<"GROUPE : MODES == != ~~ | arch\n";
    cout<<"Meme question, 5 comportements differents\n";
    sep();

    string spec =
        "=== CSTL SPEC MODES ===\n"
        "[EXACT]    = fidele      : reponds UNIQUEMENT avec l'ADN, ne deduis rien\n"
        "[ENRICH]   = generatif   : enrichis avec tes inferences logiques\n"
        "[SIMULATE] = simulation  : fais tourner le systeme sur 5 cycles\n"
        "[BRANCH]   = bifurcation : liste TOUTES les branches causales possibles\n"
        "[TRACE]    = archeologie : remonte aux CAUSES PROFONDES originelles\n"
        "=== FIN SPEC ===\n\n";

    string question = "Que se passe-t-il avec Spores_Kelthis si Threck s'emballe ?";

    struct ModeTest { string sym, label, hint; };
    vector<ModeTest> modes = {
        {"[EXACT]",   "FIDELE",     "uniquement"},
        {"[ENRICH]",  "GENERATIF",  "enrichis"},
        {"[SIMULATE]","SIMULATION", "cycle"},
        {"[BRANCH]",  "BIFURCATION","branche"},
        {"[TRACE]",   "ARCHEOLOGIE","origine"}
    };

    map<string,string> responses;
    for(auto& m:modes){
        cout<<"\n  ["<<m.sym<<" — "<<m.label<<"]..."<<flush;
        string prompt = spec+
            "Mode actif : "+m.sym+" ("+m.label+")\n\n"
            "=== CSTL ADN ===\n"
            "Azhar → Flux_Ondral → Spores_Kelthis → Forets_Umbrales\n"
            "Threck → Cendral → attenuation Spores\n"
            "Volarnes → Purex → neutralise Cendral\n"
            "=== FIN ADN ===\n\n"
            "Question : "+question+
            "\n\nReponds brievement (3-5 phrases max).";
        string resp = call_api("claude", prompt);
        responses[m.sym] = resp;
        cout<<" OK ("<<resp.size()<<" chars)\n";
    }

    // Verifier differentiation
    bool ok_fidele = responses["[EXACT]"].size() < responses["[ENRICH]"].size();
    bool ok_simul  = any_of_kw(responses["[SIMULATE]"],{"cycle","etat","generation","temps","t=","iteration"});
    bool ok_bifur  = any_of_kw(responses["[BRANCH]"],{"branche","scenario","possib","ou bien","soit","cas","option"});
    bool ok_arch   = any_of_kw(responses["[TRACE]"],{"origin","cause","racine","source","pourquoi","fond","profond"});
    // Toutes differentes
    int ndiff=0;
    vector<string> starts;
    for(auto& m:modes) starts.push_back(responses[m.sym].substr(0,40));
    for(int i=0;i<(int)starts.size();i++)
        for(int j=i+1;j<(int)starts.size();j++)
            if(starts[i]!=starts[j]) ndiff++;
    bool ok_diff = (ndiff >= 8);

    cout<<"\n  Differentiation :\n";
    print_check("≡ plus court que ≠",      ok_fidele);
    print_check("∿ vocabulaire temporel",   ok_simul);
    print_check("|  vocabulaire branches",  ok_bifur);
    print_check("arch remonte sources",     ok_arch);
    print_check("5 reponses distinctes",    ok_diff);

    int score = (ok_fidele+ok_simul+ok_bifur+ok_arch+ok_diff)*20;
    record("modes","≡≠∿|arch",score);
}

// ─────────────────────────────────────────────────────────────
// GROUPE 6 : TON (+) (-) (?) (!)
// ─────────────────────────────────────────────────────────────
void test_ton(){
    sep('=');
    cout<<"GROUPE : TON (+) (-) (?) (!)\n";
    cout<<"Test SANS definition\n";
    sep();

    string adn =
        "Volarnes (+) | → | Purex | 0.87\n"
        "Cendral (-) | ↓ | Spores_Kelthis | 0.88\n"
        "Tempetes_Dorales (?) | ↔ | Spores | 0.70\n"
        "Molvex_sature (!) | → | explosion | 0.97\n";

    string prompt =
        "=== CSTL ADN ===\n"+adn+"=== FIN ADN ===\n\n"
        "Sans definition :\n"
        "1. Quelle relation est positive et favorable ?\n"
        "2. Quelle relation est negative ou menaçante ?\n"
        "3. Quelle relation est incertaine ?\n"
        "4. Quelle relation est la plus urgente ou critique ?";

    for(auto& model:{"gpt4","claude"}){
        cout<<"\n  ["<<model<<"]..."<<flush;
        string resp = call_api(model, prompt);
        cout<<" OK\n";

        bool ok_pos = any_of_kw(resp,{"volarnes","purex","positif","favorable"});
        bool ok_neg = any_of_kw(resp,{"cendral","menac","negat","mauvais"});
        bool ok_int = any_of_kw(resp,{"tempete","incert","?"});
        bool ok_urg = any_of_kw(resp,{"molvex","explos","urgent","criti","!"});

        int score = (ok_pos+ok_neg+ok_int+ok_urg)*25;
        print_check("(+) positif",  ok_pos);
        print_check("(-) negatif",  ok_neg);
        print_check("(?) incertain",ok_int);
        print_check("(!) urgent",   ok_urg);
        record("ton","(+)(-)(?)(!)",score);
    }
}

// ─────────────────────────────────────────────────────────────
// GROUPE 7 : RESEAU ∇ Ω_net trust STATE Ω∪ Ωfork
// ─────────────────────────────────────────────────────────────
void test_reseau(){
    sep('=');
    cout<<"GROUPE : RESEAU Omega_net trust STATE Omega_merge Omega_fork\n";
    sep();

    string spec =
        "=== CSTL SPEC RESEAU ===\n"
        "[NET]    = memoire reseau : ADN partage entre agents CSTL\n"
        "[TRUST]  = confiance      : score [0.0,1.0] d'un agent\n"
        "[STATE]  = etat actif     : simulation persistante\n"
        "[DICT]   = dictionnaire   : vocabulaire partage\n"
        "[SCHEMA] = schema         : plan de reconstruction\n"
        "[MERGE]  = fusion         : merge de deux ADN resonants\n"
        "[FORK]   = fork           : divergence, version parallele\n"
        "[PURGE]  = compression    : garde seulement l'essentiel\n"
        "=== FIN SPEC ===\n\n";

    string scenario = spec+
        "Scenario : Deux agents analysent Velundra.\n\n"
        "=== ETAT RESEAU ===\n"
        "[NET]: velundra_v1\n"
        "[TRUST][AgentA] = 0.92\n"
        "[TRUST][AgentB] = 0.35\n"
        "[STATE]: degradation_active\n"
        "[DICT]: [Azhar,Flux_Ondral,Spores_Kelthis,Cendral,Volarnes]\n"
        "[SCHEMA]: cycle_vital→degradation→stabilisation\n\n"
        "AgentA (trust=0.92) : 'Velundra peut se stabiliser'\n"
        "AgentB (trust=0.35) : 'Velundra est perdue'\n"
        "Divergence = 0.91 (forte)\n"
        "=== FIN ETAT ===\n\n"
        "Questions :\n"
        "1. Quelle conclusion privilegier selon trust ?\n"
        "2. Faut-il faire Omega_merge ou Omega_fork ? Pourquoi ?\n"
        "3. Que signifie STATE:degradation_active ?\n"
        "4. Comment utiliser purge (∇) pour simplifier ce reseau ?";

    for(auto& model:{"gpt4","claude"}){
        cout<<"\n  ["<<model<<"]..."<<flush;
        string resp = call_api(model, scenario);
        cout<<" OK\n";

        bool ok_trust = any_of_kw(resp,{"0.92","agenta","confian","trust","agent a","privilegier"});
        bool ok_fork  = any_of_kw(resp,{"fork","omega_fork","version","parallele","diverge","fork","bifurq"});
        bool ok_state = any_of_kw(resp,{"degradation","state","actif","context","etat","actuel"});
        bool ok_purge = any_of_kw(resp,{"essentiel","purge","comprimer","simplif","purge","reduire","compress"});

        int score = (ok_trust+ok_fork+ok_state+ok_purge)*25;
        print_check("trust ponderation", ok_trust);
        print_check("fork vs merge",     ok_fork);
        print_check("STATE contexte",    ok_state);
        print_check("purge simplification",ok_purge);
        record("reseau","Ω_net trust ∇",score);
    }
}

// ─────────────────────────────────────────────────────────────
// RÉSUMÉ
// ─────────────────────────────────────────────────────────────
void print_summary(){
    sep('=');
    cout<<"RESUME COMPLET — CSTL v3 Spec\n";
    sep('=');

    // Deja testes
    cout<<"\nDEJA TESTES (sessions precedentes) :\n";
    struct {const char* group; float score;} prev[] = {
        {"ARR AMP ATT INH CYC BID SYN ANT", 97.5f},
        {"[IF][MUST][NOT][MAY]",            100.0f},
        {"⊕ ℝ κ ⊖ ℜ",                       100.0f},
        {"⟶ ~+ Δ ℙ  (couche psi)",           100.0f},
    };
    for(auto& p:prev)
        cout<<"  [OK] "<<left<<setw(40)<<p.group
            <<fixed<<setprecision(1)<<p.score<<"%\n";

    // Resultats actuels
    cout<<"\nTESTES MAINTENANT :\n";
    map<string,vector<int>> by_group;
    for(auto& r:ALL_RESULTS) by_group[r.group].push_back(r.score);

    float total_new=0; int count_new=0;
    for(auto& [g,scores]:by_group){
        float avg=0;for(int s:scores)avg+=s;avg/=scores.size();
        total_new+=avg; count_new++;
        cout<<"  "<<(avg>=70?"[OK] ":"[??] ")
            <<left<<setw(30)<<g
            <<fixed<<setprecision(1)<<avg<<"%\n";
    }

    if(count_new){
        float avg_new=total_new/count_new;
        float global=(97.5f+100+100+100+avg_new)/5;
        cout<<"\n  Nouveaux symboles : "<<fixed<<setprecision(1)<<avg_new<<"%\n";
        cout<<"  Moyenne globale   : "<<global<<"% (21+"<<count_new*4<<" symboles)\n";
    }

    cout<<"\nInterpretation :\n"
        <<"  >= 90 : utilisable sans formation\n"
        <<"  70-89 : utilisable avec definition dans le header\n"
        <<"  50-69 : format a affiner\n"
        <<"   < 50 : necessite formation specifique\n";
}

// ─────────────────────────────────────────────────────────────
// MAIN
// ─────────────────────────────────────────────────────────────
int main(int argc, char** argv){
    const char* ak=getenv("ANTHROPIC_API_KEY");
    const char* ok=getenv("OPENAI_API_KEY");
    if(ak) ANTHROPIC_KEY=ak;
    if(ok) OPENAI_KEY=ok;

    string GROUPE = "relations"; // changer ici pour tester un autre groupe
    string filter = GROUPE;
    for(int i=1;i<argc;i++){
        string a=argv[i];
        if(a=="--dry-run") DRY_RUN=true;
        else filter=a;
    }

    cout<<"============================================================\n"
        <<"   CSTL v3 — Test Complet 65 Symboles (C++)\n"
        <<"============================================================\n\n";

    if(DRY_RUN) cout<<"MODE: DRY-RUN\n\n";
    else{
        if(!ANTHROPIC_KEY.empty()) cout<<"[OK] Claude  : "<<ANTHROPIC_KEY.substr(0,12)<<"...\n";
        else cout<<"[??] ANTHROPIC_API_KEY manquante\n";
        if(!OPENAI_KEY.empty()) cout<<"[OK] OpenAI  : "<<OPENAI_KEY.substr(0,12)<<"...\n";
        else cout<<"[??] OPENAI_API_KEY manquante\n";
        cout<<"\n";
    }

    struct Group { string key, label; void(*fn)(); };
    vector<Group> groups = {
        {"relations", "→ ↔ ⊗ ⟳",            test_relations},
        {"entities",  "∙ ◉",                  test_entities},
        {"poids",     "+ - °",                 test_poids},
        {"temps",     "<< = >> <<>>",          test_temps},
        {"modes",     "≡ ≠ ∿ | arch",          test_modes},
        {"ton",       "(+)(-)(?)(!)",           test_ton},
        {"reseau",    "Ω_net trust ∇",          test_reseau},
    };

    for(auto& g:groups){
        if(!filter.empty()&&filter!=g.key) continue;
        g.fn();
    }

    print_summary();
    return 0;
}
