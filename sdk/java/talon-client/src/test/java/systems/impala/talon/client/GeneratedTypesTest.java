package systems.impala.talon.client;

import org.junit.jupiter.api.Test;
import talon.v1.Api;

import static org.junit.jupiter.api.Assertions.assertEquals;

final class GeneratedTypesTest {
    @Test
    void generatedV1TypesAreAvailable() {
        Api.ListResourcesRequest request = Api.ListResourcesRequest.newBuilder()
            .setNs("default")
            .setKind("Agent")
            .build();

        assertEquals("default", request.getNs());
        assertEquals("Agent", request.getKind());
    }
}
