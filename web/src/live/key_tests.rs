use super::*;

#[test]
fn absent_and_undecryptable_provider_keys_have_distinct_failures() {
    let state = crate::tests::state();
    let user = Identity {
        owner: "example-owner".into(),
        name: "example".into(),
        session_token: None,
        password_hash: None,
    };
    let practice: Practice = serde_json::from_value(json!({"provider":"gemini","language":"Spanish","level":"Beginner","voice":"Aoede","pace":"normal","input_mode":"voice","chat_id":null})).unwrap();
    assert_eq!(load_key(&state, &user, &practice).unwrap(), None);
    state
        .store
        .put_secret(&user.owner, "gemini", "damaged-data")
        .unwrap();
    assert!(load_key(&state, &user, &practice)
        .unwrap_err()
        .contains("cannot be decrypted"));
    let encrypted = crypto::seal(
        &state.config.master_key,
        &user.owner,
        "gemini",
        "synthetic-key",
    )
    .unwrap();
    state
        .store
        .put_secret(&user.owner, "gemini", &encrypted)
        .unwrap();
    assert_eq!(
        load_key(&state, &user, &practice).unwrap().as_deref(),
        Some("synthetic-key")
    );
}
