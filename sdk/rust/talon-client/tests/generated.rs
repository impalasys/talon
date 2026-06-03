use talon_client::gateway::ListAgentsRequest;

#[test]
fn generated_gateway_types_are_available() {
    let request = ListAgentsRequest {
        ns: "default".to_string(),
        ..Default::default()
    };
    assert_eq!(request.ns, "default");
}

