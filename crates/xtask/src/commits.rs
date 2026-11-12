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
/// them (`ci: add the idle measurement to PRs`).
const SCOPE_OPTIONAL: &[&str] = &["docs", "build", "ci", "chore"];

const MAX_SUBJECT: usize = 72;

pub const HELP: &str = "
Expected format:

  type(scope): imperative description, no capital letter, no final period

  Optional body: why this change.

  Refs: phase 3, P5

Types: feat, fix, refactor, perf, test, docs, build, ci, chore.
Scopes: server, agent, session, blobs, tunnel, resolve, platform, proto,
ui, scripts, packaging, ci, docs (optional for docs, build, ci, chore).
Details: section 15 of docs/PLAN.md.";

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
        errors.push("the second line must be empty".to_owned());
    }
    if !body.iter().any(|line| is_refs_line(line)) {
        errors.push(
            "missing footer: a `Refs: phase N` line (plus the prototype, `Refs: phase 3, P5`)"
                .to_owned(),
        );
    }
    if breaking
        && !body
            .iter()
            .any(|line| line.starts_with("BREAKING CHANGE: "))
    {
        errors.push("a `!` requires a `BREAKING CHANGE: ...` line in the body".to_owned());
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
            "expected format: `type(scope): description`".to_owned()
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
                    "expected format: `type(scope): description`".to_owned()
                ])
            }
        },
        None => (prefix, None),
    };

    let mut errors = Vec::new();
    if !TYPES.contains(&kind) {
        errors.push(format!("unknown type: `{kind}`"));
    }
    match scope {
        Some(scope) if !SCOPES.contains(&scope) => {
            errors.push(format!("unknown scope: `{scope}`"));
        }
        None if TYPES.contains(&kind) && !SCOPE_OPTIONAL.contains(&kind) => {
            errors.push(format!("the `{kind}` type requires a scope"));
        }
        _ => {}
    }
    errors.extend(check_description(description));
    let length = subject.chars().count();
    if length > MAX_SUBJECT {
        errors.push(format!(
            "first line too long: {length} characters, {MAX_SUBJECT} at most"
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
        return vec!["empty description".to_owned()];
    }
    if description != description.trim() {
        errors.push("extra spaces around the description".to_owned());
    }
    if description.chars().next().is_some_and(char::is_uppercase) {
        errors.push("the description starts with a lowercase letter".to_owned());
    }
    if description.trim_end().ends_with('.') {
        errors.push("no final period".to_owned());
    }
    if !starts_with_imperative(description) {
        errors.push(
            "the description starts with an imperative verb (`add`, `stop blocking`)".to_owned(),
        );
    }
    errors
}

/// True if the first word looks like an English verb in the imperative mood.
/// This is an approximation: it rules out `added`, `adding`, `adds` or
/// `fixes`, not every word that is not a verb.
fn starts_with_imperative(description: &str) -> bool {
    // Base forms that end like a third person, past tense or gerund.
    const BASE_FORMS: &[&str] = &["need", "seed", "bring", "string"];
    description.split_whitespace().next().is_some_and(|word| {
        // Case is checked separately.
        let word = word.trim_end_matches([',', ';']).to_lowercase();
        let word = word.as_str();
        if word.is_empty() || !word.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
            return false;
        }
        if BASE_FORMS.contains(&word) {
            return true;
        }
        let third_person = word.ends_with('s')
            && !["ss", "us", "is"]
                .iter()
                .any(|ending| word.ends_with(ending));
        !(third_person || word.ends_with("ed") || word.ends_with("ing"))
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
    fn plan_examples_accepted() {
        for subject in [
            "feat(blobs): prefer direct sources over the cache",
            "fix(tunnel): reopen the stream after a relay outage",
            "test(agent): cover the rejection of a badly signed manifest",
            "ci: add the idle measurement to PRs",
            "refactor(platform)!: group Resolve paths by OS",
            "fix(agent): stop blocking shutdown during a transfer",
            "build: update dependencies",
            "docs(docs): describe the repository setup",
            "fix(blobs): process ranges in order",
            "perf(blobs): seed the index from the depot",
        ] {
            assert_eq!(check_subject(subject), Vec::<String>::new(), "{subject}");
        }
    }

    #[test]
    fn messages_refused_by_the_plan() {
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
    fn subject_parsing() {
        let subject = parse_subject("refactor(platform)!: group the paths").unwrap();
        assert_eq!(
            subject,
            Subject {
                kind: "refactor",
                scope: Some("platform"),
                breaking: true,
                description: "group the paths",
            }
        );
    }

    fn single_error(subject: &str) -> String {
        let errors = check_subject(subject);
        assert_eq!(errors.len(), 1, "{subject}: {errors:?}");
        errors.into_iter().next().unwrap()
    }

    #[test]
    fn type_and_scope() {
        assert!(single_error("feature(agent): add the heartbeat").contains("unknown type"));
        assert!(single_error("feat(client): add the heartbeat").contains("unknown scope"));
        assert!(single_error("feat: add the heartbeat").contains("requires a scope"));
        assert!(single_error("feat(agent: add the heartbeat").contains("expected format"));
        assert!(single_error("feat(agent):add the heartbeat").contains("expected format"));
    }

    #[test]
    fn description_shape() {
        assert!(single_error("fix(agent): Fix the resume").contains("lowercase"));
        assert!(single_error("fix(agent): fix the resume.").contains("final period"));
        assert!(single_error("fix(agent): fixed the resume").contains("imperative"));
        assert!(single_error("fix(agent): fixes the resume").contains("imperative"));
        assert!(single_error("fix(agent): fixing the resume").contains("imperative"));
        assert!(single_error("fix(agent): résumé fix").contains("imperative"));
        assert!(single_error("fix(agent):  fix the resume").contains("spaces"));
        assert!(single_error("fix(agent): ").contains("empty"));
    }

    #[test]
    fn first_line_length() {
        let ok = format!("fix(agent): fix {}", "a".repeat(56));
        assert_eq!(ok.chars().count(), 72);
        assert_eq!(check_subject(&ok), Vec::<String>::new());
        let long = format!("{ok}a");
        assert!(single_error(&long).contains("too long"));
    }

    #[test]
    fn full_message_accepted() {
        let message = "feat(blobs): prefer direct sources over the cache\n\
                       \n\
                       The relay costs the VPS bandwidth.\n\
                       \n\
                       Refs: phase 3, P5\n\
                       Co-Authored-By: Someone <s@example.org>\n";
        assert_eq!(check_message(message), Vec::<String>::new());
    }

    #[test]
    fn footer_required() {
        let errors = check_message("feat(blobs): prefer direct sources\n");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("Refs"));
    }

    #[test]
    fn footer_forms() {
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
            "Refs: phase three",
            "refs: phase 3",
            "Refs: #12",
            "Refs: phase 3,P5",
        ] {
            assert!(!is_refs_line(line), "{line}");
        }
    }

    #[test]
    fn second_line_empty() {
        let errors = check_message("fix(agent): fix the resume\nRefs: phase 3\n");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("second line"));
    }

    #[test]
    fn breaking_change() {
        let subject = "refactor(proto)!: rename the hash field";
        let errors = check_message(&format!("{subject}\n\nRefs: phase 3\n"));
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("BREAKING CHANGE"));

        let message =
            format!("{subject}\n\nBREAKING CHANGE: older agents are refused.\n\nRefs: phase 3\n");
        assert_eq!(check_message(&message), Vec::<String>::new());
    }

    #[test]
    fn git_comments_ignored() {
        let message = "# Please enter the commit message for your changes.\n\
                       fix(agent): fix the resume\n\
                       \n\
                       Refs: phase 3\n\
                       # ------------------------ >8 ------------------------\n\
                       diff --git a/x b/x\n\
                       Refs: anything\n";
        assert_eq!(check_message(message), Vec::<String>::new());
    }

    #[test]
    fn exempt_authors() {
        assert!(is_exempt_author("dependabot[bot]"));
        assert!(!is_exempt_author("Lea"));
    }
}
