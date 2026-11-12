//! Convention for commit messages and PR titles.
//!
//! Reference: section 15 of `docs/PLAN.md`.

pub const TYPES: &[&str] = &[
    "feat", "fix", "refactor", "perf", "test", "docs", "build", "ci", "chore",
];

pub const SCOPES: &[&str] = &[
    "server",
    "agent",
    "session",
    "blobs",
    "tunnel",
    "resolve",
    "platform",
    "proto",
    "ui",
    "scripts",
    "packaging",
    "ci",
    "docs",
];

/// Types that often touch the whole repository: the scope is optional for
/// them (`ci: ajouter la mesure au repos sur les PR`).
const SCOPE_OPTIONAL: &[&str] = &["docs", "build", "ci", "chore"];

const MAX_SUBJECT: usize = 72;

pub const HELP: &str = "
Format attendu :

  type(portée): description à l'infinitif, sans majuscule ni point final

  Corps facultatif : pourquoi ce changement.

  Refs: phase 3, P5

Types : feat, fix, refactor, perf, test, docs, build, ci, chore.
Portées : server, agent, session, blobs, tunnel, resolve, platform, proto,
ui, scripts, packaging, ci, docs (facultative pour docs, build, ci, chore).
Détail : section 15 de docs/PLAN.md.";

/// Automatic dependency updates keep their own format.
pub fn is_exempt_author(author: &str) -> bool {
    author == "dependabot[bot]"
}

/// Checks the first line of a commit, or a PR title.
pub fn check_subject(subject: &str) -> Vec<String> {
    parse_subject(subject).err().unwrap_or_default()
}

/// Checks a full commit message.
pub fn check_message(message: &str) -> Vec<String> {
    // Git comment lines are not part of the message.
    let lines: Vec<&str> = message
        .lines()
        .take_while(|line| !line.starts_with("# ------------------------ >8"))
        .filter(|line| !line.starts_with('#'))
        .collect();
    let mut lines = lines.into_iter().skip_while(|line| line.trim().is_empty());
    let subject = lines.next().unwrap_or_default();
    let body: Vec<&str> = lines.collect();

    let (mut errors, breaking) = match parse_subject(subject) {
        Ok(subject) => (Vec::new(), subject.breaking),
        Err(errors) => (errors, false),
    };
    if body.first().is_some_and(|line| !line.trim().is_empty()) {
        errors.push("la deuxième ligne doit être vide".to_owned());
    }
    if !body.iter().any(|line| is_refs_line(line)) {
        errors.push(
            "pied de message absent : une ligne `Refs: phase N` (et le prototype, `Refs: phase 3, P5`)"
                .to_owned(),
        );
    }
    if breaking
        && !body
            .iter()
            .any(|line| line.starts_with("BREAKING CHANGE: "))
    {
        errors.push("un `!` demande une ligne `BREAKING CHANGE: ...` dans le corps".to_owned());
    }
    errors
}

#[derive(Debug, PartialEq, Eq)]
struct Subject<'a> {
    kind: &'a str,
    scope: Option<&'a str>,
    breaking: bool,
    description: &'a str,
}

fn parse_subject(subject: &str) -> Result<Subject<'_>, Vec<String>> {
    let Some((prefix, description)) = subject.split_once(": ") else {
        return Err(vec![
            "format attendu : `type(portée): description`".to_owned()
        ]);
    };
    let (prefix, breaking) = match prefix.strip_suffix('!') {
        Some(prefix) => (prefix, true),
        None => (prefix, false),
    };
    let (kind, scope) = match prefix.split_once('(') {
        Some((kind, rest)) => match rest.strip_suffix(')') {
            Some(scope) => (kind, Some(scope)),
            None => {
                return Err(vec![
                    "format attendu : `type(portée): description`".to_owned()
                ])
            }
        },
        None => (prefix, None),
    };

    let mut errors = Vec::new();
    if !TYPES.contains(&kind) {
        errors.push(format!("type inconnu : `{kind}`"));
    }
    match scope {
        Some(scope) if !SCOPES.contains(&scope) => {
            errors.push(format!("portée inconnue : `{scope}`"));
        }
        None if TYPES.contains(&kind) && !SCOPE_OPTIONAL.contains(&kind) => {
            errors.push(format!("le type `{kind}` demande une portée"));
        }
        _ => {}
    }
    errors.extend(check_description(description));
    let length = subject.chars().count();
    if length > MAX_SUBJECT {
        errors.push(format!(
            "première ligne trop longue : {length} caractères, {MAX_SUBJECT} au plus"
        ));
    }

    if errors.is_empty() {
        Ok(Subject {
            kind,
            scope,
            breaking,
            description,
        })
    } else {
        Err(errors)
    }
}

fn check_description(description: &str) -> Vec<String> {
    let mut errors = Vec::new();
    if description.trim().is_empty() {
        return vec!["description vide".to_owned()];
    }
    if description != description.trim() {
        errors.push("espaces en trop autour de la description".to_owned());
    }
    if description.chars().next().is_some_and(char::is_uppercase) {
        errors.push("la description commence par une minuscule".to_owned());
    }
    if description.trim_end().ends_with('.') {
        errors.push("pas de point final".to_owned());
    }
    if !starts_with_infinitive(description) {
        errors.push(
            "la description commence par un verbe à l'infinitif (`ajouter`, `ne plus bloquer`)"
                .to_owned(),
        );
    }
    errors
}

/// True if the first word, after a negation or a pronoun, has a French
/// infinitive ending. This is an approximation: it rules out `update`,
/// `corrections` or `ajout de`, not every word that is not a verb.
fn starts_with_infinitive(description: &str) -> bool {
    const SKIPPED: &[&str] = &["ne", "pas", "plus", "jamais", "rien", "se"];
    const ENDINGS: &[&str] = &["er", "ir", "ïr", "re"];
    description
        .split_whitespace()
        .find(|word| !SKIPPED.contains(word))
        .is_some_and(|word| {
            let word = word.trim_end_matches([',', ';']);
            word.chars()
                .all(|c| c.is_alphabetic() || c == '\'' || c == '-')
                && ENDINGS.iter().any(|ending| word.ends_with(ending))
        })
}

/// `Refs: phase 3`, `Refs: phase 3, P5`, `Refs: P1, P2`.
fn is_refs_line(line: &str) -> bool {
    let Some(items) = line.strip_prefix("Refs: ") else {
        return false;
    };
    items.split(", ").all(|item| {
        let number = item
            .strip_prefix("phase ")
            .or_else(|| item.strip_prefix('P'));
        number
            .is_some_and(|n| !n.is_empty() && n.len() <= 2 && n.bytes().all(|b| b.is_ascii_digit()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exemples_du_plan_acceptes() {
        for subject in [
            "feat(blobs): choisir les sources directes avant le cache",
            "fix(tunnel): rouvrir le flux après une coupure du relais",
            "test(agent): couvrir le refus d'un manifeste mal signé",
            "ci: ajouter la mesure au repos sur les PR",
            "refactor(platform)!: regrouper les chemins Resolve par OS",
            "fix(agent): ne plus bloquer l'arrêt pendant un transfert",
            "build: mettre à jour les dépendances",
            "docs(docs): décrire la mise en place du dépôt",
        ] {
            assert_eq!(check_subject(subject), Vec::<String>::new(), "{subject}");
        }
    }

    #[test]
    fn messages_refuses_par_le_plan() {
        for subject in [
            "update",
            "wip",
            "fix bug",
            "corrections",
            "Merge pull request",
        ] {
            assert!(!check_subject(subject).is_empty(), "{subject}");
        }
    }

    #[test]
    fn decoupage_du_titre() {
        let subject = parse_subject("refactor(platform)!: regrouper les chemins").unwrap();
        assert_eq!(
            subject,
            Subject {
                kind: "refactor",
                scope: Some("platform"),
                breaking: true,
                description: "regrouper les chemins",
            }
        );
    }

    fn single_error(subject: &str) -> String {
        let errors = check_subject(subject);
        assert_eq!(errors.len(), 1, "{subject} : {errors:?}");
        errors.into_iter().next().unwrap()
    }

    #[test]
    fn type_et_portee() {
        assert!(single_error("feature(agent): ajouter le signe de vie").contains("type inconnu"));
        assert!(single_error("feat(client): ajouter le signe de vie").contains("portée inconnue"));
        assert!(single_error("feat: ajouter le signe de vie").contains("demande une portée"));
        assert!(single_error("feat(agent: ajouter le signe de vie").contains("format attendu"));
        assert!(single_error("feat(agent):ajouter le signe de vie").contains("format attendu"));
    }

    #[test]
    fn forme_de_la_description() {
        assert!(single_error("fix(agent): Corriger la reprise").contains("minuscule"));
        assert!(single_error("fix(agent): corriger la reprise.").contains("point final"));
        assert!(single_error("fix(agent): correction de la reprise").contains("infinitif"));
        assert!(single_error("fix(agent): fixed resume").contains("infinitif"));
        assert!(single_error("fix(agent):  corriger la reprise").contains("espaces"));
        assert!(single_error("fix(agent): ").contains("vide"));
    }

    #[test]
    fn longueur_de_la_premiere_ligne() {
        let ok = format!("fix(agent): corriger {}", "é".repeat(51));
        assert_eq!(ok.chars().count(), 72);
        assert_eq!(check_subject(&ok), Vec::<String>::new());
        let long = format!("{ok}é");
        assert!(single_error(&long).contains("trop longue"));
    }

    #[test]
    fn message_complet_accepte() {
        let message = "feat(blobs): choisir les sources directes avant le cache\n\
                       \n\
                       Le relais coûte de la bande passante au VPS.\n\
                       \n\
                       Refs: phase 3, P5\n\
                       Co-Authored-By: Quelqu'un <q@example.org>\n";
        assert_eq!(check_message(message), Vec::<String>::new());
    }

    #[test]
    fn pied_de_message_obligatoire() {
        let errors = check_message("feat(blobs): choisir les sources directes\n");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("Refs"));
    }

    #[test]
    fn formes_du_pied_de_message() {
        for line in [
            "Refs: phase 0",
            "Refs: phase 3, P5",
            "Refs: P1, P2",
            "Refs: phase 4, P5, P10",
        ] {
            assert!(is_refs_line(line), "{line}");
        }
        for line in [
            "Refs:",
            "Refs: ",
            "Refs: phase",
            "Refs: phase trois",
            "refs: phase 3",
            "Refs: #12",
            "Refs: phase 3,P5",
        ] {
            assert!(!is_refs_line(line), "{line}");
        }
    }

    #[test]
    fn deuxieme_ligne_vide() {
        let errors = check_message("fix(agent): corriger la reprise\nRefs: phase 3\n");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("deuxième ligne"));
    }

    #[test]
    fn changement_cassant() {
        let subject = "refactor(proto)!: renommer le champ des empreintes";
        let errors = check_message(&format!("{subject}\n\nRefs: phase 3\n"));
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("BREAKING CHANGE"));

        let message =
            format!("{subject}\n\nBREAKING CHANGE: agents antérieurs refusés.\n\nRefs: phase 3\n");
        assert_eq!(check_message(&message), Vec::<String>::new());
    }

    #[test]
    fn commentaires_de_git_ignores() {
        let message = "# Saisissez le message de validation\n\
                       fix(agent): corriger la reprise\n\
                       \n\
                       Refs: phase 3\n\
                       # ------------------------ >8 ------------------------\n\
                       diff --git a/x b/x\n\
                       Refs: n'importe quoi\n";
        assert_eq!(check_message(message), Vec::<String>::new());
    }

    #[test]
    fn auteurs_dispenses() {
        assert!(is_exempt_author("dependabot[bot]"));
        assert!(!is_exempt_author("Léa"));
    }
}
