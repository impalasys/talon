package systems.impala.talon.client;

import org.junit.jupiter.api.Test;
import talon.gateway.Gateway;

import static org.junit.jupiter.api.Assertions.assertEquals;

final class GeneratedTypesTest {
    @Test
    void generatedGatewayTypesAreAvailable() {
        Gateway.ListResourcesRequest request = Gateway.ListResourcesRequest.newBuilder()
            .setNs("default")
            .setKind("Agent")
            .build();

        assertEquals("default", request.getNs());
        assertEquals("Agent", request.getKind());
    }
}
