use identity::{
    auth::{AuthenticationService, RegistrationService},
    user::{NewUser, UserRepository, UserStatus},
};
use sqlx::PgPool;

#[sqlx::test(migrations = "./migrations")]
async fn registration_login_logout_and_logout_all_work(pool: PgPool) {
    let registration = RegistrationService::new(pool.clone());
    let authentication = AuthenticationService::new(pool.clone());

    let user = registration
        .register(
            &NewUser {
                username: "auth_flow_user".to_string(),
                email: "auth-flow@example.test".to_string(),
            },
            "correct-horse-battery",
        )
        .await
        .unwrap();

    assert_eq!(user.status, UserStatus::Active);

    let first_session = authentication
        .login("auth_flow_user", "correct-horse-battery")
        .await
        .unwrap();
    assert!(
        authentication
            .validate_token(&first_session.token)
            .await
            .unwrap()
            .is_some()
    );

    let revoked = authentication
        .logout(first_session.session.id, user.id)
        .await
        .unwrap();
    assert!(revoked);
    assert!(
        authentication
            .validate_token(&first_session.token)
            .await
            .unwrap()
            .is_none()
    );

    let second_session = authentication
        .login("auth-flow@example.test", "correct-horse-battery")
        .await
        .unwrap();
    let third_session = authentication
        .login("auth_flow_user", "correct-horse-battery")
        .await
        .unwrap();

    let revoked_count = authentication.logout_all(user.id).await.unwrap();
    assert_eq!(revoked_count, 2);
    assert!(
        authentication
            .validate_token(&second_session.token)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        authentication
            .validate_token(&third_session.token)
            .await
            .unwrap()
            .is_none()
    );

    let active_session = authentication
        .login("auth_flow_user", "correct-horse-battery")
        .await
        .unwrap();
    assert!(
        authentication
            .validate_token(&active_session.token)
            .await
            .unwrap()
            .is_some()
    );

    UserRepository::new(pool)
        .update_status(user.id, UserStatus::Disabled)
        .await
        .unwrap()
        .expect("user should exist");

    assert!(
        authentication
            .validate_token(&active_session.token)
            .await
            .unwrap()
            .is_none(),
        "disabling a user must invalidate their existing sessions"
    );
}
