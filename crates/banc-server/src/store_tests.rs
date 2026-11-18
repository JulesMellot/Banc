use banc_proto::keys::SecretKey;
use banc_proto::machine::ResolveEdition;

use super::*;
use crate::db;

const NOW: i64 = 1_800_000_000;

fn key(seed: u8) -> PublicKey {
    SecretKey::from_bytes(&[seed; 32]).public()
}

/// A database with one admin (id 1) and an admin key.
fn setup() -> Connection {
    let mut conn = db::open_in_memory().unwrap();
    create_admin(&conn, "jules", "hash", NOW).unwrap().unwrap();
    set_admin_key(&mut conn, &key(100), 1, NOW).unwrap();
    conn
}

fn invite(conn: &Connection, token: u8, now: i64) -> InviteRow {
    let new = NewInvite {
        token_hash: &[token; 32],
        user_name: "Léa",
        person_role: PersonRole::Editor,
        machine_role: MachineRole::Workstation,
        admin_key: &key(100),
        created_by: 1,
    };
    create_invite(conn, &new, now).unwrap()
}

fn machine(public_key: &PublicKey) -> NewMachine<'_> {
    NewMachine {
        public_key,
        name: "Station 3",
        os: Os::Macos,
        arch: "aarch64",
        agent_version: "0.1.0",
    }
}

fn event_kinds(conn: &Connection) -> Vec<String> {
    let mut statement = conn.prepare("SELECT kind FROM events ORDER BY id").unwrap();
    let rows = statement.query_map([], |row| row.get(0)).unwrap();
    rows.collect::<Result<_, _>>().unwrap()
}

#[test]
fn admin_names_are_unique() {
    let conn = db::open_in_memory().unwrap();
    assert_eq!(admin_count(&conn).unwrap(), 0);
    assert_eq!(create_admin(&conn, "jules", "h1", NOW).unwrap(), Some(1));
    assert_eq!(create_admin(&conn, "jules", "h2", NOW).unwrap(), None);
    assert_eq!(admin_count(&conn).unwrap(), 1);
    let (admin, hash) = admin_by_name(&conn, "jules").unwrap().unwrap();
    assert_eq!(admin.name, "jules");
    assert_eq!(hash, "h1");
    assert!(admin_by_name(&conn, "paul").unwrap().is_none());
}

#[test]
fn sessions_expire_and_are_cleaned_up() {
    let conn = setup();
    create_session(&conn, &[1; 32], 1, NOW, NOW + 100).unwrap();
    assert_eq!(
        session_admin(&conn, &[1; 32], NOW + 99)
            .unwrap()
            .unwrap()
            .id,
        1
    );
    assert!(session_admin(&conn, &[1; 32], NOW + 100).unwrap().is_none());
    assert!(session_admin(&conn, &[2; 32], NOW).unwrap().is_none());

    // Creating a session later removes the expired one.
    create_session(&conn, &[2; 32], 1, NOW + 200, NOW + 300).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM admin_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);

    delete_session(&conn, &[2; 32]).unwrap();
    assert!(session_admin(&conn, &[2; 32], NOW + 200).unwrap().is_none());
}

#[test]
fn admin_key_replaced() {
    let mut conn = setup();
    assert_eq!(admin_key(&conn).unwrap(), Some(key(100)));
    set_admin_key(&mut conn, &key(101), 1, NOW).unwrap();
    assert_eq!(admin_key(&conn).unwrap(), Some(key(101)));
    assert_eq!(
        event_kinds(&conn),
        ["admin_created", "admin_key_set", "admin_key_set"]
    );
}

#[test]
fn invite_status_rules() {
    use InviteStatus::*;
    let expires = NOW + 10;
    assert_eq!(invite_status(None, None, expires, true, NOW), Pending);
    assert_eq!(invite_status(None, None, expires, true, expires), Expired);
    assert_eq!(invite_status(None, None, expires, false, NOW), Obsolete);
    assert_eq!(invite_status(Some(NOW), None, expires, true, NOW), Used);
    assert_eq!(invite_status(None, Some(NOW), expires, true, NOW), Revoked);
    // A used or revoked invitation does not become "expired".
    assert_eq!(
        invite_status(Some(NOW), None, expires, true, expires + 1),
        Used
    );
    assert_eq!(
        invite_status(None, Some(NOW), expires, false, expires + 1),
        Revoked
    );
}

#[test]
fn invites_listed_with_their_status() {
    let mut conn = setup();
    let first = invite(&conn, 1, NOW);
    assert_eq!(first.expires_at, NOW + INVITE_LIFETIME_SECS);
    let second = invite(&conn, 2, NOW + 1);
    assert!(revoke_invite(&conn, second.id, 1, NOW + 2).unwrap());
    assert!(!revoke_invite(&conn, second.id, 1, NOW + 2).unwrap());
    assert!(!revoke_invite(&conn, 999, 1, NOW + 2).unwrap());

    let listed = list_invites(&conn, NOW + 3).unwrap();
    let statuses: Vec<_> = listed.iter().map(|i| (i.id, i.status)).collect();
    assert_eq!(
        statuses,
        [
            (second.id, InviteStatus::Revoked),
            (first.id, InviteStatus::Pending)
        ]
    );

    let later = list_invites(&conn, NOW + INVITE_LIFETIME_SECS).unwrap();
    assert_eq!(later[1].status, InviteStatus::Expired);

    set_admin_key(&mut conn, &key(101), 1, NOW + 4).unwrap();
    let after_key_change = list_invites(&conn, NOW + 5).unwrap();
    assert_eq!(after_key_change[1].status, InviteStatus::Obsolete);
}

#[test]
fn enrollment_consumes_the_invitation() {
    let mut conn = setup();
    let invite = invite(&conn, 1, NOW);
    let enrolled = enroll(&mut conn, &[1; 32], &machine(&key(1)), NOW + 10).unwrap();
    assert_eq!(
        enrolled,
        Enrolled {
            machine_id: 1,
            machine_role: MachineRole::Workstation,
            person_role: PersonRole::Editor,
            admin_key: key(100),
        }
    );

    let listed = list_invites(&conn, NOW + 11).unwrap();
    assert_eq!(listed[0].id, invite.id);
    assert_eq!(listed[0].status, InviteStatus::Used);
    assert_eq!(listed[0].machine_id, Some(1));

    let machines = list_machines(&conn).unwrap();
    assert_eq!(machines.len(), 1);
    let row = &machines[0];
    assert_eq!(row.name, "Station 3");
    assert_eq!(row.public_key, key(1).to_string());
    assert_eq!(row.user_name, "Léa");
    assert_eq!(row.person_role, PersonRole::Editor);
    assert_eq!(row.enrolled_at, NOW + 10);
    assert_eq!(row.last_seen_at, None);

    // The same token never enrolls a second machine.
    let again = enroll(&mut conn, &[1; 32], &machine(&key(2)), NOW + 12);
    assert!(matches!(
        again,
        Err(EnrollError::Refused(EnrollRefusal::InviteUsed))
    ));
    assert_eq!(list_machines(&conn).unwrap().len(), 1);
    assert!(event_kinds(&conn).contains(&"machine_enrolled".to_owned()));
}

#[test]
fn enrollment_refusals() {
    let mut conn = setup();
    let refusal = |conn: &mut Connection, token: u8, public_key: &PublicKey, now: i64| match enroll(
        conn,
        &[token; 32],
        &machine(public_key),
        now,
    ) {
        Err(EnrollError::Refused(refusal)) => refusal,
        other => panic!("expected a refusal, got {other:?}"),
    };

    assert_eq!(
        refusal(&mut conn, 9, &key(1), NOW),
        EnrollRefusal::UnknownInvite
    );

    invite(&conn, 1, NOW);
    let expiry = NOW + INVITE_LIFETIME_SECS;
    assert_eq!(
        refusal(&mut conn, 1, &key(1), expiry),
        EnrollRefusal::InviteExpired
    );

    let revoked = invite(&conn, 2, NOW);
    revoke_invite(&conn, revoked.id, 1, NOW).unwrap();
    assert_eq!(
        refusal(&mut conn, 2, &key(1), NOW),
        EnrollRefusal::InviteRevoked
    );

    // A key already enrolled, even revoked since, is refused.
    invite(&conn, 3, NOW);
    invite(&conn, 4, NOW);
    let enrolled = enroll(&mut conn, &[3; 32], &machine(&key(1)), NOW).unwrap();
    assert_eq!(
        refusal(&mut conn, 4, &key(1), NOW),
        EnrollRefusal::KeyAlreadyEnrolled
    );
    revoke_machine(&conn, enrolled.machine_id, 1, NOW).unwrap();
    assert_eq!(
        refusal(&mut conn, 4, &key(1), NOW),
        EnrollRefusal::KeyAlreadyEnrolled
    );

    // After an admin key change, older invitations no longer work.
    set_admin_key(&mut conn, &key(101), 1, NOW).unwrap();
    assert_eq!(
        refusal(&mut conn, 4, &key(2), NOW),
        EnrollRefusal::AdminKeyChanged
    );

    // Nothing was written by the refusals.
    assert_eq!(list_machines(&conn).unwrap().len(), 1);
}

#[test]
fn hello_and_reports_update_the_machine() {
    let mut conn = setup();
    invite(&conn, 1, NOW);
    let id = enroll(&mut conn, &[1; 32], &machine(&key(1)), NOW)
        .unwrap()
        .machine_id;

    let mut inventory = Inventory {
        hostname: "station-3.local".to_owned(),
        os_version: "macOS 26.1".to_owned(),
        resolve: Some(ResolveInstall {
            edition: Some(ResolveEdition::Studio),
            version: "21.1".to_owned(),
        }),
    };
    record_hello(&mut conn, id, "0.1.1", &inventory, NOW + 5).unwrap();
    let row = active_machine(&conn, &key(1)).unwrap().unwrap();
    assert_eq!(row.agent_version, "0.1.1");
    assert_eq!(row.last_seen_at, Some(NOW + 5));
    assert_eq!(row.hostname.as_deref(), Some("station-3.local"));
    assert_eq!(row.resolve, inventory.resolve);

    touch_machine(&conn, id, NOW + 300).unwrap();
    let row = active_machine(&conn, &key(1)).unwrap().unwrap();
    assert_eq!(row.last_seen_at, Some(NOW + 300));

    // Same inventory again: no new event. Resolve updated: one event.
    record_report(&mut conn, id, &inventory, NOW + 301).unwrap();
    inventory.resolve = Some(ResolveInstall {
        edition: Some(ResolveEdition::Studio),
        version: "21.2".to_owned(),
    });
    record_report(&mut conn, id, &inventory, NOW + 302).unwrap();
    let changes = event_kinds(&conn)
        .iter()
        .filter(|kind| *kind == "resolve_changed")
        .count();
    // Once at the first hello (nothing to 21.1), once for 21.1 to 21.2.
    assert_eq!(changes, 2);

    // The version is kept when the edition is unknown.
    inventory.resolve = Some(ResolveInstall {
        edition: None,
        version: "21.2".to_owned(),
    });
    record_report(&mut conn, id, &inventory, NOW + 303).unwrap();
    assert_eq!(
        active_machine(&conn, &key(1)).unwrap().unwrap().resolve,
        inventory.resolve
    );

    inventory.resolve = None;
    record_report(&mut conn, id, &inventory, NOW + 303).unwrap();
    assert_eq!(
        active_machine(&conn, &key(1)).unwrap().unwrap().resolve,
        None
    );
}

#[test]
fn revoked_machine_is_no_longer_active() {
    let mut conn = setup();
    invite(&conn, 1, NOW);
    let id = enroll(&mut conn, &[1; 32], &machine(&key(1)), NOW)
        .unwrap()
        .machine_id;
    assert!(active_machine(&conn, &key(1)).unwrap().is_some());

    assert_eq!(revoke_machine(&conn, id, 1, NOW + 1).unwrap(), Some(key(1)));
    assert_eq!(revoke_machine(&conn, id, 1, NOW + 2).unwrap(), None);
    assert_eq!(revoke_machine(&conn, 999, 1, NOW + 2).unwrap(), None);
    assert!(active_machine(&conn, &key(1)).unwrap().is_none());

    let listed = list_machines(&conn).unwrap();
    assert_eq!(listed[0].revoked_at, Some(NOW + 1));
}
