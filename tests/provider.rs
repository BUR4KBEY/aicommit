use std::collections::BTreeMap;

use aicommit::{
    ai::{
        AiEngine, ChatMessage, GenerationRequest, anthropic::AnthropicEngine, engine_from_config,
        openai_compat::OpenAiCompatEngine,
    },
    config::Config,
    generator,
    git::CommitInfo,
    prompt::{build_pr_messages, initial_messages},
    token::{count_messages, count_tokens},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_string_contains, header, header_exists, method, path},
};

#[tokio::test]
async fn openai_compatible_engine_reads_chat_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [
                { "message": { "content": "<think>hidden</think>\nfeat: add cli" } }
            ]
        })))
        .mount(&server)
        .await;

    let config = Config {
        api_key: Some("key".to_owned()),
        api_url: Some(format!("{}/v1", server.uri())),
        ..Config::default()
    };
    let engine = OpenAiCompatEngine::new(config).unwrap();
    let response = engine
        .generate_commit_message(&[ChatMessage::user("diff")])
        .await
        .unwrap();

    assert_eq!(response, "feat: add cli");
}

#[tokio::test]
async fn azure_openai_engine_uses_api_key_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/openai/v1/chat/completions"))
        .and(header("api-key", "key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [
                { "message": { "content": "feat: add azure openai" } }
            ]
        })))
        .mount(&server)
        .await;

    let config = Config {
        ai_provider: "azure-openai".to_owned(),
        api_key: Some("key".to_owned()),
        api_url: Some(format!("{}/openai/v1", server.uri())),
        ..Config::default()
    };
    let engine = OpenAiCompatEngine::new(config).unwrap();
    let response = engine
        .generate_commit_message(&[ChatMessage::user("diff")])
        .await
        .unwrap();

    assert_eq!(response, "feat: add azure openai");
}

#[test]
fn engine_from_config_accepts_supported_providers() {
    let anthropic = Config {
        ai_provider: "anthropic".to_owned(),
        model: "claude-sonnet-4-20250514".to_owned(),
        ..Config::default()
    };
    let groq = Config {
        ai_provider: "groq".to_owned(),
        model: "llama-3.1-8b-instant".to_owned(),
        ..Config::default()
    };
    let ollama = Config {
        ai_provider: "ollama".to_owned(),
        model: "llama3.2".to_owned(),
        ..Config::default()
    };
    let claude = Config {
        ai_provider: "claude-code".to_owned(),
        model: "default".to_owned(),
        ..Config::default()
    };
    let codex = Config {
        ai_provider: "codex".to_owned(),
        model: "default".to_owned(),
        ..Config::default()
    };
    let copilot = Config {
        ai_provider: "copilot".to_owned(),
        model: "default".to_owned(),
        ..Config::default()
    };

    let apple = Config {
        ai_provider: "apple".to_owned(),
        model: "default".to_owned(),
        ..Config::default()
    };

    let opencode_go = Config {
        ai_provider: "opencode-go".to_owned(),
        model: "glm-5.3-flash".to_owned(),
        ..Config::default()
    };

    assert!(engine_from_config(&anthropic).is_ok());
    assert!(engine_from_config(&groq).is_ok());
    assert!(engine_from_config(&ollama).is_ok());
    assert!(engine_from_config(&claude).is_ok());
    assert!(engine_from_config(&codex).is_ok());
    assert!(engine_from_config(&copilot).is_ok());
    assert!(engine_from_config(&apple).is_ok());
    assert!(engine_from_config(&opencode_go).is_ok());
}

#[tokio::test]
async fn groq_engine_uses_openai_compatible_base_url_and_bearer_auth() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/openai/v1/chat/completions"))
        .and(header("authorization", "Bearer key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [
                { "message": { "content": "feat: add groq support" } }
            ]
        })))
        .mount(&server)
        .await;

    let config = Config {
        ai_provider: "groq".to_owned(),
        api_key: Some("key".to_owned()),
        api_url: Some(format!("{}/openai/v1", server.uri())),
        model: "llama-3.1-8b-instant".to_owned(),
        ..Config::default()
    };
    let engine = OpenAiCompatEngine::new(config).unwrap();
    let response = engine
        .generate_commit_message(&[ChatMessage::user("diff")])
        .await
        .unwrap();

    assert_eq!(response, "feat: add groq support");
}

#[tokio::test]
async fn ollama_engine_uses_openai_compatible_base_url_without_api_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [
                { "message": { "content": "feat: add ollama support" } }
            ]
        })))
        .mount(&server)
        .await;

    let config = Config {
        ai_provider: "ollama".to_owned(),
        api_url: Some(format!("{}/v1", server.uri())),
        model: "llama3.2".to_owned(),
        ..Config::default()
    };
    let engine = OpenAiCompatEngine::new(config).unwrap();
    let response = engine
        .generate_commit_message(&[ChatMessage::user("diff")])
        .await
        .unwrap();

    assert_eq!(response, "feat: add ollama support");
}

async fn recorded_header(server: &MockServer, name: &str) -> Option<String> {
    server
        .received_requests()
        .await
        .expect("request recording")
        .iter()
        .flat_map(|request| request.headers.get(name).cloned())
        .next()
        .map(|value| value.to_str().unwrap_or_default().to_owned())
}

#[tokio::test]
async fn opencode_go_engine_sends_a_uuid4_session_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header_exists("x-opencode-session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [
                { "message": { "content": "feat: add opencode go support" } }
            ]
        })))
        .mount(&server)
        .await;

    let config = Config {
        ai_provider: "opencode-go".to_owned(),
        api_key: Some("key".to_owned()),
        api_url: Some(format!("{}/v1", server.uri())),
        model: "glm-5.3-flash".to_owned(),
        ..Config::default()
    };
    let engine = OpenAiCompatEngine::new(config).unwrap();
    let response = engine
        .generate_commit_message(&[ChatMessage::user("diff")])
        .await
        .unwrap();

    assert_eq!(response, "feat: add opencode go support");
    let session = recorded_header(&server, "x-opencode-session")
        .await
        .expect("session header sent");
    let parsed = uuid::Uuid::parse_str(&session).expect("uuid session id");
    assert_eq!(parsed.get_version_num(), 4);
}

#[tokio::test]
async fn opencode_go_session_id_is_stable_within_one_process() {
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    for server in [&first, &second] {
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [
                    { "message": { "content": "feat: add opencode go support" } }
                ]
            })))
            .mount(server)
            .await;
    }

    let config = |server: &MockServer| Config {
        ai_provider: "opencode-go".to_owned(),
        api_url: Some(format!("{}/v1", server.uri())),
        model: "glm-5.3-flash".to_owned(),
        ..Config::default()
    };

    OpenAiCompatEngine::new(config(&first))
        .unwrap()
        .generate_commit_message(&[ChatMessage::user("diff")])
        .await
        .unwrap();
    OpenAiCompatEngine::new(config(&second))
        .unwrap()
        .generate_commit_message(&[ChatMessage::user("diff")])
        .await
        .unwrap();

    let first_id = recorded_header(&first, "x-opencode-session")
        .await
        .expect("session header sent");
    let second_id = recorded_header(&second, "x-opencode-session")
        .await
        .expect("session header sent");
    assert_eq!(first_id, second_id);
}

#[tokio::test]
async fn opencode_go_custom_session_header_wins() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [
                { "message": { "content": "feat: add opencode go support" } }
            ]
        })))
        .mount(&server)
        .await;

    let config = Config {
        ai_provider: "opencode-go".to_owned(),
        api_url: Some(format!("{}/v1", server.uri())),
        api_custom_headers: BTreeMap::from([(
            "X-OpenCode-Session".to_owned(),
            "user-supplied".to_owned(),
        )]),
        model: "glm-5.3-flash".to_owned(),
        ..Config::default()
    };
    OpenAiCompatEngine::new(config)
        .unwrap()
        .generate_commit_message(&[ChatMessage::user("diff")])
        .await
        .unwrap();

    let sessions: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .flat_map(|request| {
            request
                .headers
                .get_all("x-opencode-session")
                .iter()
                .cloned()
        })
        .map(|value| value.to_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(sessions, vec!["user-supplied".to_owned()]);
}

#[tokio::test]
async fn other_providers_send_no_session_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [
                { "message": { "content": "feat: add openai support" } }
            ]
        })))
        .mount(&server)
        .await;

    let config = Config {
        ai_provider: "openai".to_owned(),
        api_url: Some(format!("{}/v1", server.uri())),
        ..Config::default()
    };
    OpenAiCompatEngine::new(config)
        .unwrap()
        .generate_commit_message(&[ChatMessage::user("diff")])
        .await
        .unwrap();

    assert!(
        recorded_header(&server, "x-opencode-session")
            .await
            .is_none()
    );
}

#[tokio::test]
async fn anthropic_engine_uses_messages_api_and_flattens_text_blocks() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "key"))
        .and(header("anthropic-version", "2023-06-01"))
        .and(body_string_contains(
            "\"system\":\"system rules\\n\\nreview context\"",
        ))
        .and(body_string_contains("\"role\":\"user\""))
        .and(body_string_contains("\"role\":\"assistant\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "content": [
                { "type": "thinking", "text": "hidden" },
                { "type": "text", "text": "<think>hidden</think>\nfeat: add anthropic support" },
                { "type": "text", "text": "- wire provider defaults" }
            ]
        })))
        .mount(&server)
        .await;

    let config = Config {
        ai_provider: "anthropic".to_owned(),
        api_key: Some("key".to_owned()),
        api_url: Some(format!("{}/v1", server.uri())),
        model: "claude-sonnet-4-20250514".to_owned(),
        ..Config::default()
    };
    let engine = AnthropicEngine::new(config).unwrap();
    let response = engine
        .generate_commit_message(&[
            ChatMessage::system("system rules"),
            ChatMessage::user("diff"),
            ChatMessage::assistant("assistant example"),
            ChatMessage::system("review context"),
        ])
        .await
        .unwrap();

    assert_eq!(
        response,
        "feat: add anthropic support\n- wire provider defaults"
    );
}

#[tokio::test]
async fn anthropic_engine_reports_cap_truncation_and_honors_a_per_call_cap() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_string_contains("\"max_tokens\":2048"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "content": [{ "type": "text", "text": "{\"groups\":[" }],
            "stop_reason": "max_tokens"
        })))
        .mount(&server)
        .await;

    let config = Config {
        ai_provider: "anthropic".to_owned(),
        api_key: Some("key".to_owned()),
        api_url: Some(format!("{}/v1", server.uri())),
        tokens_max_output: 500,
        ..Config::default()
    };
    let engine = AnthropicEngine::new(config).unwrap();
    let generation = engine
        .generate_with_options(
            &[ChatMessage::user("diff")],
            &GenerationRequest {
                max_output_tokens: Some(2048),
            },
        )
        .await
        .unwrap();

    assert_eq!(generation.text, "{\"groups\":[");
    assert!(
        generation.truncated,
        "stop_reason max_tokens must surface as truncation"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn generate_pull_request_synthesizes_chunked_diff() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("This is diff chunk"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [
                { "message": { "content": "- Capture one slice of the PR diff" } }
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("Partial summaries from cumulative PR diff"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [
                { "message": { "content": "feat(cli): generate PR drafts\n\n## Summary\n- Combine chunk summaries into one PR draft\n\n## Testing\n- cargo test" } }
            ]
        })))
        .mount(&server)
        .await;

    let config = Config {
        api_key: Some("key".to_owned()),
        api_url: Some(format!("{}/v1", server.uri())),
        tokens_max_input: 500,
        tokens_max_output: 80,
        ..Config::default()
    };
    let commits = vec![CommitInfo {
        hash: "abc123".to_owned(),
        subject: "feat(cli): add PR command".to_owned(),
        body: String::new(),
    }];
    let files = vec!["src/cli.rs".to_owned()];
    let prompt_tokens = count_messages(
        &build_pr_messages(
            &config,
            "",
            "",
            "main",
            Some("feature/pr"),
            None,
            &commits,
            &files,
        )
        .unwrap(),
    );
    let available = config
        .tokens_max_input
        .saturating_sub(config.tokens_max_output)
        .saturating_sub(prompt_tokens)
        .saturating_sub(20)
        .max(1);
    let mut diff = "diff --git a/src/cli.rs b/src/cli.rs\n".to_owned();
    while count_tokens(&diff) <= available {
        diff.push_str("@@\n+new line in chunked diff\n");
    }

    let draft = generator::generate_pull_request(
        &config,
        &diff,
        "",
        "main",
        Some("feature/pr"),
        None,
        &commits,
        &files,
        None,
    )
    .await
    .unwrap();

    assert_eq!(draft.title, "feat(cli): generate PR drafts");
    assert!(draft.body.contains("## Summary"));
    assert!(draft.body.contains("## Testing"));
}

#[tokio::test]
async fn generate_commit_message_synthesizes_chunked_diff() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("This is diff chunk"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [
                { "message": { "content": "capture one slice of the staged diff" } }
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains(
            "Partial summaries from a large staged diff",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [
                { "message": { "content": "feat(cli): handle very large staged diffs" } }
            ]
        })))
        .mount(&server)
        .await;

    let config = Config {
        api_key: Some("key".to_owned()),
        api_url: Some(format!("{}/v1", server.uri())),
        // The commit system prompt plus few-shot examples is ~600 tokens, so
        // the cap must leave real chunk budget on top of that.
        tokens_max_input: 3000,
        tokens_max_output: 80,
        ..Config::default()
    };
    let files = vec!["src/cli.rs".to_owned()];
    let prompt_tokens = count_messages(&initial_messages(&config, false, "", &files).unwrap());
    let available = config
        .tokens_max_input
        .saturating_sub(config.tokens_max_output)
        .saturating_sub(prompt_tokens)
        .saturating_sub(20)
        .max(1);
    let mut diff = "diff --git a/src/cli.rs b/src/cli.rs\n".to_owned();
    while count_tokens(&diff) <= available {
        diff.push_str("@@\n+new line in chunked diff\n");
    }

    let events = std::sync::Mutex::new(Vec::new());
    let progress = |event: generator::GenerationProgress| events.lock().unwrap().push(event);

    let message =
        generator::generate_commit_message(&config, &diff, false, "", &files, Some(&progress))
            .await
            .unwrap();

    assert_eq!(message, "feat(cli): handle very large staged diffs");

    let events = events.into_inner().unwrap();
    assert!(events.contains(&generator::GenerationProgress::Splitting));
    assert!(events.iter().any(
        |event| matches!(event, generator::GenerationProgress::Chunk { total, .. } if *total > 1)
    ));
    assert!(events.contains(&generator::GenerationProgress::Synthesizing));
}
