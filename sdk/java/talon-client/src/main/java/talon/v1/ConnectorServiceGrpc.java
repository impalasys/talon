package talon.v1;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 * <pre>
 * ConnectorService is implemented by Talon's gateway for callbacks from an
 * external connector service. The connector service owns provider-specific
 * webhooks and OAuth/runtime details; Talon owns routing into Sessions and
 * Channels after a normalized event is accepted here.
 * </pre>
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class ConnectorServiceGrpc {

  private ConnectorServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.v1.ConnectorService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<talon.external.Connectors.ConnectorMessageEvent,
      talon.external.Connectors.ConnectorMessageEventResponse> getIngestMessageEventMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "IngestMessageEvent",
      requestType = talon.external.Connectors.ConnectorMessageEvent.class,
      responseType = talon.external.Connectors.ConnectorMessageEventResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.external.Connectors.ConnectorMessageEvent,
      talon.external.Connectors.ConnectorMessageEventResponse> getIngestMessageEventMethod() {
    io.grpc.MethodDescriptor<talon.external.Connectors.ConnectorMessageEvent, talon.external.Connectors.ConnectorMessageEventResponse> getIngestMessageEventMethod;
    if ((getIngestMessageEventMethod = ConnectorServiceGrpc.getIngestMessageEventMethod) == null) {
      synchronized (ConnectorServiceGrpc.class) {
        if ((getIngestMessageEventMethod = ConnectorServiceGrpc.getIngestMessageEventMethod) == null) {
          ConnectorServiceGrpc.getIngestMessageEventMethod = getIngestMessageEventMethod =
              io.grpc.MethodDescriptor.<talon.external.Connectors.ConnectorMessageEvent, talon.external.Connectors.ConnectorMessageEventResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "IngestMessageEvent"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.external.Connectors.ConnectorMessageEvent.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.external.Connectors.ConnectorMessageEventResponse.getDefaultInstance()))
              .setSchemaDescriptor(new ConnectorServiceMethodDescriptorSupplier("IngestMessageEvent"))
              .build();
        }
      }
    }
    return getIngestMessageEventMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.external.Connectors.ConnectorStatusEvent,
      talon.external.Connectors.ConnectorAckResponse> getReportStatusMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ReportStatus",
      requestType = talon.external.Connectors.ConnectorStatusEvent.class,
      responseType = talon.external.Connectors.ConnectorAckResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.external.Connectors.ConnectorStatusEvent,
      talon.external.Connectors.ConnectorAckResponse> getReportStatusMethod() {
    io.grpc.MethodDescriptor<talon.external.Connectors.ConnectorStatusEvent, talon.external.Connectors.ConnectorAckResponse> getReportStatusMethod;
    if ((getReportStatusMethod = ConnectorServiceGrpc.getReportStatusMethod) == null) {
      synchronized (ConnectorServiceGrpc.class) {
        if ((getReportStatusMethod = ConnectorServiceGrpc.getReportStatusMethod) == null) {
          ConnectorServiceGrpc.getReportStatusMethod = getReportStatusMethod =
              io.grpc.MethodDescriptor.<talon.external.Connectors.ConnectorStatusEvent, talon.external.Connectors.ConnectorAckResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ReportStatus"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.external.Connectors.ConnectorStatusEvent.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.external.Connectors.ConnectorAckResponse.getDefaultInstance()))
              .setSchemaDescriptor(new ConnectorServiceMethodDescriptorSupplier("ReportStatus"))
              .build();
        }
      }
    }
    return getReportStatusMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static ConnectorServiceStub newStub(io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ConnectorServiceStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ConnectorServiceStub>() {
        @java.lang.Override
        public ConnectorServiceStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ConnectorServiceStub(channel, callOptions);
        }
      };
    return ConnectorServiceStub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports all types of calls on the service
   */
  public static ConnectorServiceBlockingV2Stub newBlockingV2Stub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ConnectorServiceBlockingV2Stub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ConnectorServiceBlockingV2Stub>() {
        @java.lang.Override
        public ConnectorServiceBlockingV2Stub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ConnectorServiceBlockingV2Stub(channel, callOptions);
        }
      };
    return ConnectorServiceBlockingV2Stub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static ConnectorServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ConnectorServiceBlockingStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ConnectorServiceBlockingStub>() {
        @java.lang.Override
        public ConnectorServiceBlockingStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ConnectorServiceBlockingStub(channel, callOptions);
        }
      };
    return ConnectorServiceBlockingStub.newStub(factory, channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static ConnectorServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ConnectorServiceFutureStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ConnectorServiceFutureStub>() {
        @java.lang.Override
        public ConnectorServiceFutureStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ConnectorServiceFutureStub(channel, callOptions);
        }
      };
    return ConnectorServiceFutureStub.newStub(factory, channel);
  }

  /**
   * <pre>
   * ConnectorService is implemented by Talon's gateway for callbacks from an
   * external connector service. The connector service owns provider-specific
   * webhooks and OAuth/runtime details; Talon owns routing into Sessions and
   * Channels after a normalized event is accepted here.
   * </pre>
   */
  public interface AsyncService {

    /**
     * <pre>
     * IngestMessageEvent delivers one normalized provider message event to Talon.
     * Talon deduplicates under the ConnectorClass registration by event_id,
     * resolves a Connector by match_fields, and dispatches the message to the
     * resolved message consumer.
     * </pre>
     */
    default void ingestMessageEvent(talon.external.Connectors.ConnectorMessageEvent request,
        io.grpc.stub.StreamObserver<talon.external.Connectors.ConnectorMessageEventResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getIngestMessageEventMethod(), responseObserver);
    }

    /**
     * <pre>
     * ReportStatus lets the connector service report registration or provider
     * connection health without sending a message event.
     * </pre>
     */
    default void reportStatus(talon.external.Connectors.ConnectorStatusEvent request,
        io.grpc.stub.StreamObserver<talon.external.Connectors.ConnectorAckResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getReportStatusMethod(), responseObserver);
    }
  }

  /**
   * Base class for the server implementation of the service ConnectorService.
   * <pre>
   * ConnectorService is implemented by Talon's gateway for callbacks from an
   * external connector service. The connector service owns provider-specific
   * webhooks and OAuth/runtime details; Talon owns routing into Sessions and
   * Channels after a normalized event is accepted here.
   * </pre>
   */
  public static abstract class ConnectorServiceImplBase
      implements io.grpc.BindableService, AsyncService {

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return ConnectorServiceGrpc.bindService(this);
    }
  }

  /**
   * A stub to allow clients to do asynchronous rpc calls to service ConnectorService.
   * <pre>
   * ConnectorService is implemented by Talon's gateway for callbacks from an
   * external connector service. The connector service owns provider-specific
   * webhooks and OAuth/runtime details; Talon owns routing into Sessions and
   * Channels after a normalized event is accepted here.
   * </pre>
   */
  public static final class ConnectorServiceStub
      extends io.grpc.stub.AbstractAsyncStub<ConnectorServiceStub> {
    private ConnectorServiceStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ConnectorServiceStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ConnectorServiceStub(channel, callOptions);
    }

    /**
     * <pre>
     * IngestMessageEvent delivers one normalized provider message event to Talon.
     * Talon deduplicates under the ConnectorClass registration by event_id,
     * resolves a Connector by match_fields, and dispatches the message to the
     * resolved message consumer.
     * </pre>
     */
    public void ingestMessageEvent(talon.external.Connectors.ConnectorMessageEvent request,
        io.grpc.stub.StreamObserver<talon.external.Connectors.ConnectorMessageEventResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getIngestMessageEventMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * ReportStatus lets the connector service report registration or provider
     * connection health without sending a message event.
     * </pre>
     */
    public void reportStatus(talon.external.Connectors.ConnectorStatusEvent request,
        io.grpc.stub.StreamObserver<talon.external.Connectors.ConnectorAckResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getReportStatusMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   * A stub to allow clients to do synchronous rpc calls to service ConnectorService.
   * <pre>
   * ConnectorService is implemented by Talon's gateway for callbacks from an
   * external connector service. The connector service owns provider-specific
   * webhooks and OAuth/runtime details; Talon owns routing into Sessions and
   * Channels after a normalized event is accepted here.
   * </pre>
   */
  public static final class ConnectorServiceBlockingV2Stub
      extends io.grpc.stub.AbstractBlockingStub<ConnectorServiceBlockingV2Stub> {
    private ConnectorServiceBlockingV2Stub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ConnectorServiceBlockingV2Stub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ConnectorServiceBlockingV2Stub(channel, callOptions);
    }

    /**
     * <pre>
     * IngestMessageEvent delivers one normalized provider message event to Talon.
     * Talon deduplicates under the ConnectorClass registration by event_id,
     * resolves a Connector by match_fields, and dispatches the message to the
     * resolved message consumer.
     * </pre>
     */
    public talon.external.Connectors.ConnectorMessageEventResponse ingestMessageEvent(talon.external.Connectors.ConnectorMessageEvent request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getIngestMessageEventMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * ReportStatus lets the connector service report registration or provider
     * connection health without sending a message event.
     * </pre>
     */
    public talon.external.Connectors.ConnectorAckResponse reportStatus(talon.external.Connectors.ConnectorStatusEvent request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getReportStatusMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do limited synchronous rpc calls to service ConnectorService.
   * <pre>
   * ConnectorService is implemented by Talon's gateway for callbacks from an
   * external connector service. The connector service owns provider-specific
   * webhooks and OAuth/runtime details; Talon owns routing into Sessions and
   * Channels after a normalized event is accepted here.
   * </pre>
   */
  public static final class ConnectorServiceBlockingStub
      extends io.grpc.stub.AbstractBlockingStub<ConnectorServiceBlockingStub> {
    private ConnectorServiceBlockingStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ConnectorServiceBlockingStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ConnectorServiceBlockingStub(channel, callOptions);
    }

    /**
     * <pre>
     * IngestMessageEvent delivers one normalized provider message event to Talon.
     * Talon deduplicates under the ConnectorClass registration by event_id,
     * resolves a Connector by match_fields, and dispatches the message to the
     * resolved message consumer.
     * </pre>
     */
    public talon.external.Connectors.ConnectorMessageEventResponse ingestMessageEvent(talon.external.Connectors.ConnectorMessageEvent request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getIngestMessageEventMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * ReportStatus lets the connector service report registration or provider
     * connection health without sending a message event.
     * </pre>
     */
    public talon.external.Connectors.ConnectorAckResponse reportStatus(talon.external.Connectors.ConnectorStatusEvent request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getReportStatusMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do ListenableFuture-style rpc calls to service ConnectorService.
   * <pre>
   * ConnectorService is implemented by Talon's gateway for callbacks from an
   * external connector service. The connector service owns provider-specific
   * webhooks and OAuth/runtime details; Talon owns routing into Sessions and
   * Channels after a normalized event is accepted here.
   * </pre>
   */
  public static final class ConnectorServiceFutureStub
      extends io.grpc.stub.AbstractFutureStub<ConnectorServiceFutureStub> {
    private ConnectorServiceFutureStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ConnectorServiceFutureStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ConnectorServiceFutureStub(channel, callOptions);
    }

    /**
     * <pre>
     * IngestMessageEvent delivers one normalized provider message event to Talon.
     * Talon deduplicates under the ConnectorClass registration by event_id,
     * resolves a Connector by match_fields, and dispatches the message to the
     * resolved message consumer.
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.external.Connectors.ConnectorMessageEventResponse> ingestMessageEvent(
        talon.external.Connectors.ConnectorMessageEvent request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getIngestMessageEventMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * ReportStatus lets the connector service report registration or provider
     * connection health without sending a message event.
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.external.Connectors.ConnectorAckResponse> reportStatus(
        talon.external.Connectors.ConnectorStatusEvent request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getReportStatusMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_INGEST_MESSAGE_EVENT = 0;
  private static final int METHODID_REPORT_STATUS = 1;

  private static final class MethodHandlers<Req, Resp> implements
      io.grpc.stub.ServerCalls.UnaryMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ServerStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ClientStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.BidiStreamingMethod<Req, Resp> {
    private final AsyncService serviceImpl;
    private final int methodId;

    MethodHandlers(AsyncService serviceImpl, int methodId) {
      this.serviceImpl = serviceImpl;
      this.methodId = methodId;
    }

    @java.lang.Override
    @java.lang.SuppressWarnings("unchecked")
    public void invoke(Req request, io.grpc.stub.StreamObserver<Resp> responseObserver) {
      switch (methodId) {
        case METHODID_INGEST_MESSAGE_EVENT:
          serviceImpl.ingestMessageEvent((talon.external.Connectors.ConnectorMessageEvent) request,
              (io.grpc.stub.StreamObserver<talon.external.Connectors.ConnectorMessageEventResponse>) responseObserver);
          break;
        case METHODID_REPORT_STATUS:
          serviceImpl.reportStatus((talon.external.Connectors.ConnectorStatusEvent) request,
              (io.grpc.stub.StreamObserver<talon.external.Connectors.ConnectorAckResponse>) responseObserver);
          break;
        default:
          throw new AssertionError();
      }
    }

    @java.lang.Override
    @java.lang.SuppressWarnings("unchecked")
    public io.grpc.stub.StreamObserver<Req> invoke(
        io.grpc.stub.StreamObserver<Resp> responseObserver) {
      switch (methodId) {
        default:
          throw new AssertionError();
      }
    }
  }

  public static final io.grpc.ServerServiceDefinition bindService(AsyncService service) {
    return io.grpc.ServerServiceDefinition.builder(getServiceDescriptor())
        .addMethod(
          getIngestMessageEventMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.external.Connectors.ConnectorMessageEvent,
              talon.external.Connectors.ConnectorMessageEventResponse>(
                service, METHODID_INGEST_MESSAGE_EVENT)))
        .addMethod(
          getReportStatusMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.external.Connectors.ConnectorStatusEvent,
              talon.external.Connectors.ConnectorAckResponse>(
                service, METHODID_REPORT_STATUS)))
        .build();
  }

  private static abstract class ConnectorServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    ConnectorServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return talon.v1.Connectors.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("ConnectorService");
    }
  }

  private static final class ConnectorServiceFileDescriptorSupplier
      extends ConnectorServiceBaseDescriptorSupplier {
    ConnectorServiceFileDescriptorSupplier() {}
  }

  private static final class ConnectorServiceMethodDescriptorSupplier
      extends ConnectorServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final java.lang.String methodName;

    ConnectorServiceMethodDescriptorSupplier(java.lang.String methodName) {
      this.methodName = methodName;
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.MethodDescriptor getMethodDescriptor() {
      return getServiceDescriptor().findMethodByName(methodName);
    }
  }

  private static volatile io.grpc.ServiceDescriptor serviceDescriptor;

  public static io.grpc.ServiceDescriptor getServiceDescriptor() {
    io.grpc.ServiceDescriptor result = serviceDescriptor;
    if (result == null) {
      synchronized (ConnectorServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new ConnectorServiceFileDescriptorSupplier())
              .addMethod(getIngestMessageEventMethod())
              .addMethod(getReportStatusMethod())
              .build();
        }
      }
    }
    return result;
  }
}
