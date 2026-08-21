use futures_util::StreamExt;
use provider_adapters::capabilities::{
    Credential, ProviderContentBlock, ProviderMessage, ProviderRequest,
};
use provider_adapters::http_stream::{stream_chat_completions, stream_responses};
use provider_adapters::providers::{anthropic::AnthropicAdapter, gemini::GeminiAdapter};
use provider_adapters::{ProviderAdapter, ProviderEvent};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

type ProviderStream = std::pin::Pin<Box<dyn futures_util::Stream<Item = ProviderEvent> + Send>>;

fn request() -> ProviderRequest {
    ProviderRequest {
        model: "fixture-model".into(),
        messages: vec![ProviderMessage {
            role: "user".into(),
            content: vec![ProviderContentBlock::Text { text: "hi".into() }],
        }],
        system_prompt: None,
        tools: None,
        max_tokens: Some(16),
        temperature: None,
        stream: true,
        structured_output: None,
        controls: Default::default(),
    }
}

fn credential(base_url: String) -> Credential {
    Credential {
        api_key: "fixture-secret".into(),
        base_url: Some(base_url),
        proxy_url: None,
        key_id: Some("fixture-key".into()),
        provider_type: None,
        project_id: None,
    }
}

async fn read_request(socket: &mut TcpStream) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let (header_end, content_length) = loop {
        let count = socket.read(&mut buffer).await.expect("read request");
        assert!(count > 0, "client closed before sending request headers");
        request.extend_from_slice(&buffer[..count]);
        if let Some(offset) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            let header_end = offset + 4;
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length:")
                        .or_else(|| line.strip_prefix("Content-Length:"))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            break (header_end, content_length);
        }
    };
    while request.len() < header_end + content_length {
        let count = socket.read(&mut buffer).await.expect("read request body");
        assert!(count > 0, "client closed before sending request body");
        request.extend_from_slice(&buffer[..count]);
    }
}

async fn truncated_server(payload: &'static str) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        socket.write_all(response.as_bytes()).await.unwrap();
    });
    (format!("http://{address}"), task)
}

fn assert_incomplete(events: &[ProviderEvent]) {
    assert!(!events
        .iter()
        .any(|event| matches!(event, ProviderEvent::Completed { .. })));
    assert!(matches!(
        events.last(),
        Some(ProviderEvent::Error(error)) if error.code == "incomplete_stream" && error.retryable
    ));
}

async fn collect(stream: ProviderStream) -> Vec<ProviderEvent> {
    stream.collect::<Vec<_>>().await
}

#[tokio::test]
async fn chat_completions_rejects_eof_without_done() {
    let (base_url, server) = truncated_server(
        "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n",
    )
    .await;
    let stream = stream_chat_completions(&reqwest::Client::new(), &base_url, "key", request())
        .await
        .unwrap();
    assert_incomplete(&collect(stream).await);
    server.await.unwrap();
}

#[tokio::test]
async fn responses_rejects_eof_without_response_terminal() {
    let (base_url, server) = truncated_server(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
    )
    .await;
    let stream = stream_responses(&reqwest::Client::new(), &base_url, "key", request())
        .await
        .unwrap();
    assert_incomplete(&collect(stream).await);
    server.await.unwrap();
}

#[tokio::test]
async fn anthropic_rejects_eof_without_message_stop() {
    let (base_url, server) = truncated_server(
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n\n",
    )
    .await;
    let stream = AnthropicAdapter::new()
        .stream(request(), credential(base_url))
        .await
        .unwrap();
    assert_incomplete(&collect(stream).await);
    server.await.unwrap();
}

#[tokio::test]
async fn gemini_rejects_eof_without_finish_reason() {
    let (base_url, server) = truncated_server(
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"partial\"}]}}]}\n\n",
    )
    .await;
    let stream = GeminiAdapter::new()
        .stream(request(), credential(base_url))
        .await
        .unwrap();
    assert_incomplete(&collect(stream).await);
    server.await.unwrap();
}

#[tokio::test]
async fn dropping_chat_stream_closes_the_upstream_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        let payload =
            "data: {\"choices\":[{\"delta\":{\"content\":\"first\"},\"finish_reason\":null}]}\n\n";
        let headers = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n";
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket
            .write_all(format!("{:x}\r\n{payload}\r\n", payload.len()).as_bytes())
            .await
            .unwrap();
        socket.flush().await.unwrap();

        let mut byte = [0_u8; 1];
        match tokio::time::timeout(std::time::Duration::from_secs(2), socket.read(&mut byte)).await
        {
            Ok(Ok(0)) | Ok(Err(_)) => true,
            _ => false,
        }
    });

    let mut stream = stream_chat_completions(
        &reqwest::Client::new(),
        &format!("http://{address}"),
        "key",
        request(),
    )
    .await
    .unwrap();
    assert!(matches!(
        stream.next().await,
        Some(ProviderEvent::TextDelta(text)) if text == "first"
    ));
    drop(stream);

    assert!(
        server.await.unwrap(),
        "upstream socket remained open after cancellation"
    );
}
