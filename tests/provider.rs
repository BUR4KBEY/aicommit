use aicommit::{
    ai::{
        AiEngine, ChatMessage, anthropic::AnthropicEngine, engine_from_config,
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
    matchers::{body_string_contains, header, method, path},
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

    assert!(engine_from_config(&anthropic).is_ok());
    assert!(engine_from_config(&groq).is_ok());
    assert!(engine_from_config(&ollama).is_ok());
    assert!(engine_from_config(&claude).is_ok());
    assert!(engine_from_config(&codex).is_ok());
    assert!(engine_from_config(&copilot).is_ok());
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
