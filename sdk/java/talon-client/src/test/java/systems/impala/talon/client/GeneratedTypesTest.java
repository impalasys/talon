package systems.impala.talon.client;

import org.junit.jupiter.api.Test;
import talon.gateway.Gateway;

import static org.junit.jupiter.api.Assertions.assertEquals;

final class GeneratedTypesTest {
    @Test
    void generatedGatewayTypesAreAvailable() {
        Gateway.ListAgentsRequest request = Gateway.ListAgentsRequest.newBuilder()
            .setNs("default")
            .build();

        assertEquals("default", request.getNs());
    }
}

