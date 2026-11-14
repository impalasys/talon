package systems.impala.talon.client;

import io.grpc.Channel;
import io.grpc.ClientCall;
import io.grpc.MethodDescriptor;
import org.junit.jupiter.api.Test;
import talon.harness.Llm;
import talon.v1.NamespaceServiceGrpc;
import talon.v1.Resources;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;

final class GeneratedTypesTest {
    @Test
    void generatedV1TypesAreAvailable() {
        Resources.ListResourcesRequest request = Resources.ListResourcesRequest.newBuilder()
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

    @Test
    void chatContentPartByteRangeRoundTrips() throws Exception {
        Llm.ByteRange range = Llm.ByteRange.newBuilder().setStart(0).setEnd(5).build();
        Llm.ChatContentPart part = Llm.ChatContentPart.newBuilder()
            .setText("héllo wörld")
            .setByteRange(range)
            .build();
        Llm.ChatContentPart decoded = Llm.ChatContentPart.parseFrom(part.toByteArray());
        assertEquals(part, decoded);
        assertEquals(5L, decoded.getByteRange().getEnd());
    }

    @Test
    @SuppressWarnings("deprecation")
    void toolOutputDeprecatedByteRangeReceiptRoundTrips() throws Exception {
        Llm.ToolOutput output = Llm.ToolOutput.newBuilder()
            .addContentParts(Llm.ChatContentPart.newBuilder().setText("abc"))
            .setSummary("s")
            .setByteRange(Llm.ToolOutputByteRange.newBuilder()
                .setStart(0)
                .setEnd(3)
                .setNextByte(3))
            .build();
        Llm.ToolOutput decoded = Llm.ToolOutput.parseFrom(output.toByteArray());
        assertEquals(output, decoded);
        assertEquals(3L, decoded.getByteRange().getNextByte());
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
