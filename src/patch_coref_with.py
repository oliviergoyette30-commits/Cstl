import re
from pathlib import Path

SEMANTIC_RS = Path("semantic.rs")

NEW_FUNCTION = '''
    /// R8 (coref_with) — spec §13 : coref_with=eXXX doit référencer un
    /// id=eXXX déclaré dans un bloc DEFINE du même payload. Sinon warning
    /// (pas erreur — loi de Postel, cf. spec ligne 497-498).
    fn check_coref_with(&self) -> Vec<SemanticError> {
        let mut errors = Vec::new();
        let mut declared_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        for block in self.blocks {
            if block.name == "DEFINE" || block.name.starts_with("DEFINE_") || block.name.starts_with("DEFINE:") {
                for f in &block.fields {
                    if f.name == "id" || f.name == "\\u{03B9}" {
                        declared_ids.insert(f.value.clone());
                    }
                }
            }
        }

        for block in self.blocks {
            if block.name == "DEFINE" || block.name.starts_with("DEFINE_") || block.name.starts_with("DEFINE:") {
                for f in &block.fields {
                    if f.name == "coref_with" {
                        if !declared_ids.contains(&f.value) {
                            errors.push(SemanticError {
                                code: "R8".to_string(),
                                message: format!(
                                    "coref_with='{}' ne correspond à aucun id= déclaré dans ce payload (ligne {})",
                                    f.value, block.line
                                ),
                                line: block.line,
                            });
                        }
                    }
                }
            }
        }
        errors
    }
'''

TEST_BLOCK = '''
    #[test]
    fn test_r8_coref_with_valid_reference_no_warning() {
        let blocks = vec![
            Block {
                name: "DEFINE".to_string(),
                fields: vec![
                    Field { name: "id".to_string(), type_hint: None, value: "e001".to_string(), line: 1 },
                    Field { name: "name".to_string(), type_hint: None, value: "alice".to_string(), line: 1 },
                    Field { name: "type".to_string(), type_hint: None, value: "human".to_string(), line: 1 },
                ],
                subblocks: vec![],
                line: 1,
            },
            Block {
                name: "DEFINE".to_string(),
                fields: vec![
                    Field { name: "id".to_string(), type_hint: None, value: "e002".to_string(), line: 2 },
                    Field { name: "name".to_string(), type_hint: None, value: "elle".to_string(), line: 2 },
                    Field { name: "type".to_string(), type_hint: None, value: "human".to_string(), line: 2 },
                    Field { name: "coref_with".to_string(), type_hint: None, value: "e001".to_string(), line: 2 },
                ],
                subblocks: vec![],
                line: 2,
            },
        ];
        let data: Vec<Relation> = vec![];
        let v = SemanticValidator::new(&data, &blocks);
        assert!(!v.validate().iter().any(|e| e.code == "R8"),
            "coref_with valide ne devrait déclencher aucun warning R8");
    }

    #[test]
    fn test_r8_coref_with_undefined_reference_warns() {
        let blocks = vec![
            Block {
                name: "DEFINE".to_string(),
                fields: vec![
                    Field { name: "id".to_string(), type_hint: None, value: "e002".to_string(), line: 1 },
                    Field { name: "name".to_string(), type_hint: None, value: "elle".to_string(), line: 1 },
                    Field { name: "type".to_string(), type_hint: None, value: "human".to_string(), line: 1 },
                    Field { name: "coref_with".to_string(), type_hint: None, value: "e999".to_string(), line: 1 },
                ],
                subblocks: vec![],
                line: 1,
            },
        ];
        let data: Vec<Relation> = vec![];
        let v = SemanticValidator::new(&data, &blocks);
        assert!(v.validate().iter().any(|e| e.code == "R8"),
            "coref_with vers une entité non déclarée devrait déclencher R8");
    }
'''


def main():
    if not SEMANTIC_RS.exists():
        print(f"❌ {SEMANTIC_RS} introuvable — lance ce script depuis ~/cstl/src")
        return

    content = SEMANTIC_RS.read_text()

    if "fn check_coref_with" in content:
        print("✓ check_coref_with existe déjà — patch idempotent, rien à faire")
        return

    marker_fn = "    fn check_undefined_entity_reference(&self) -> Vec<SemanticError> {"
    if marker_fn not in content:
        print("❌ Marqueur check_undefined_entity_reference introuvable — abandon")
        return

    idx = content.index(marker_fn)
    rest = content[idx:]
    end_marker = "\n    }\n"
    end_idx_rel = rest.index(end_marker) + len(end_marker)
    insertion_point = idx + end_idx_rel

    content = content[:insertion_point] + NEW_FUNCTION + content[insertion_point:]

    validate_marker = "errors.extend(self.check_undefined_entity_reference());"
    if validate_marker not in content:
        print("❌ Marqueur validate() introuvable — abandon")
        return
    content = content.replace(
        validate_marker,
        validate_marker + "\n        errors.extend(self.check_coref_with());"
    )

    content = content.rstrip()
    if content.endswith("}"):
        last_brace = content.rfind("\n}")
        content = content[:last_brace] + TEST_BLOCK + content[last_brace:]
    content += "\n"

    SEMANTIC_RS.write_text(content)
    print("✓ check_coref_with() ajoutée après check_undefined_entity_reference")
    print("✓ Enregistrée dans validate()")
    print("✓ 2 tests ajoutés")
    print("\n→ Lance maintenant: cd ~/cstl && cargo test coref_with")


if __name__ == "__main__":
    main()
