package systems.impala.talon.client;

import io.grpc.Channel;
import io.grpc.ClientCall;
import io.grpc.MethodDescriptor;
import org.junit.jupiter.api.Test;
import talon.v1.Api;
import talon.v1.NamespaceServiceGrpc;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;

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

    @Test
    void generatedClientsetExposesServiceStubs() {
        TalonClientset clientset = TalonClientset.create(new FakeChannel());

        assertNotNull(clientset.namespaces());
        assertNotNull(clientset.resources());
        assertNotNull(clientset.sessionsAsync());
        assertNotNull(clientset.channelsAsync());
        assertNotNull(clientset.workflows());
        assertNotNull(clientset.knowledgeFuture());
        assertNotNull(clientset.authFuture());
        assertEquals(NamespaceServiceGrpc.SERVICE_NAME, clientset.namespaces().getChannel()
            .authority());
    }

    private static final class FakeChannel extends Channel {
        @Override
        public <RequestT, ResponseT> ClientCall<RequestT, ResponseT> newCall(
            MethodDescriptor<RequestT, ResponseT> methodDescriptor,
            io.grpc.CallOptions callOptions
        ) {
            throw new UnsupportedOperationException("test channel does not make calls");
        }

        @Override
        public String authority() {
            return NamespaceServiceGrpc.SERVICE_NAME;
        }
    }
}
