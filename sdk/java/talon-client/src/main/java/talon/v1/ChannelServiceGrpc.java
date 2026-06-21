package talon.v1;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class ChannelServiceGrpc {

  private ChannelServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.v1.ChannelService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<talon.v1.Api.PostChannelMessageRequest,
      talon.v1.Api.PostChannelMessageResponse> getPostMessageMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "PostMessage",
      requestType = talon.v1.Api.PostChannelMessageRequest.class,
      responseType = talon.v1.Api.PostChannelMessageResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Api.PostChannelMessageRequest,
      talon.v1.Api.PostChannelMessageResponse> getPostMessageMethod() {
    io.grpc.MethodDescriptor<talon.v1.Api.PostChannelMessageRequest, talon.v1.Api.PostChannelMessageResponse> getPostMessageMethod;
    if ((getPostMessageMethod = ChannelServiceGrpc.getPostMessageMethod) == null) {
      synchronized (ChannelServiceGrpc.class) {
        if ((getPostMessageMethod = ChannelServiceGrpc.getPostMessageMethod) == null) {
          ChannelServiceGrpc.getPostMessageMethod = getPostMessageMethod =
              io.grpc.MethodDescriptor.<talon.v1.Api.PostChannelMessageRequest, talon.v1.Api.PostChannelMessageResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "PostMessage"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.PostChannelMessageRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.PostChannelMessageResponse.getDefaultInstance()))
              .setSchemaDescriptor(new ChannelServiceMethodDescriptorSupplier("PostMessage"))
              .build();
        }
      }
    }
    return getPostMessageMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Api.GetChannelMessageRequest,
      talon.v1.Api.ChannelMessageResponse> getGetMessageMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetMessage",
      requestType = talon.v1.Api.GetChannelMessageRequest.class,
      responseType = talon.v1.Api.ChannelMessageResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Api.GetChannelMessageRequest,
      talon.v1.Api.ChannelMessageResponse> getGetMessageMethod() {
    io.grpc.MethodDescriptor<talon.v1.Api.GetChannelMessageRequest, talon.v1.Api.ChannelMessageResponse> getGetMessageMethod;
    if ((getGetMessageMethod = ChannelServiceGrpc.getGetMessageMethod) == null) {
      synchronized (ChannelServiceGrpc.class) {
        if ((getGetMessageMethod = ChannelServiceGrpc.getGetMessageMethod) == null) {
          ChannelServiceGrpc.getGetMessageMethod = getGetMessageMethod =
              io.grpc.MethodDescriptor.<talon.v1.Api.GetChannelMessageRequest, talon.v1.Api.ChannelMessageResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetMessage"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.GetChannelMessageRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.ChannelMessageResponse.getDefaultInstance()))
              .setSchemaDescriptor(new ChannelServiceMethodDescriptorSupplier("GetMessage"))
              .build();
        }
      }
    }
    return getGetMessageMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Api.ListChannelMessagesRequest,
      talon.v1.Api.ListChannelMessagesResponse> getListMessagesMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListMessages",
      requestType = talon.v1.Api.ListChannelMessagesRequest.class,
      responseType = talon.v1.Api.ListChannelMessagesResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Api.ListChannelMessagesRequest,
      talon.v1.Api.ListChannelMessagesResponse> getListMessagesMethod() {
    io.grpc.MethodDescriptor<talon.v1.Api.ListChannelMessagesRequest, talon.v1.Api.ListChannelMessagesResponse> getListMessagesMethod;
    if ((getListMessagesMethod = ChannelServiceGrpc.getListMessagesMethod) == null) {
      synchronized (ChannelServiceGrpc.class) {
        if ((getListMessagesMethod = ChannelServiceGrpc.getListMessagesMethod) == null) {
          ChannelServiceGrpc.getListMessagesMethod = getListMessagesMethod =
              io.grpc.MethodDescriptor.<talon.v1.Api.ListChannelMessagesRequest, talon.v1.Api.ListChannelMessagesResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListMessages"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.ListChannelMessagesRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.ListChannelMessagesResponse.getDefaultInstance()))
              .setSchemaDescriptor(new ChannelServiceMethodDescriptorSupplier("ListMessages"))
              .build();
        }
      }
    }
    return getListMessagesMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Api.StreamChannelEventsRequest,
      talon.events.Events.ChannelEvent> getStreamEventsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "StreamEvents",
      requestType = talon.v1.Api.StreamChannelEventsRequest.class,
      responseType = talon.events.Events.ChannelEvent.class,
      methodType = io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
  public static io.grpc.MethodDescriptor<talon.v1.Api.StreamChannelEventsRequest,
      talon.events.Events.ChannelEvent> getStreamEventsMethod() {
    io.grpc.MethodDescriptor<talon.v1.Api.StreamChannelEventsRequest, talon.events.Events.ChannelEvent> getStreamEventsMethod;
    if ((getStreamEventsMethod = ChannelServiceGrpc.getStreamEventsMethod) == null) {
      synchronized (ChannelServiceGrpc.class) {
        if ((getStreamEventsMethod = ChannelServiceGrpc.getStreamEventsMethod) == null) {
          ChannelServiceGrpc.getStreamEventsMethod = getStreamEventsMethod =
              io.grpc.MethodDescriptor.<talon.v1.Api.StreamChannelEventsRequest, talon.events.Events.ChannelEvent>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "StreamEvents"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.StreamChannelEventsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.events.Events.ChannelEvent.getDefaultInstance()))
              .setSchemaDescriptor(new ChannelServiceMethodDescriptorSupplier("StreamEvents"))
              .build();
        }
      }
    }
    return getStreamEventsMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static ChannelServiceStub newStub(io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ChannelServiceStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ChannelServiceStub>() {
        @java.lang.Override
        public ChannelServiceStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ChannelServiceStub(channel, callOptions);
        }
      };
    return ChannelServiceStub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports all types of calls on the service
   */
  public static ChannelServiceBlockingV2Stub newBlockingV2Stub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ChannelServiceBlockingV2Stub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ChannelServiceBlockingV2Stub>() {
        @java.lang.Override
        public ChannelServiceBlockingV2Stub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ChannelServiceBlockingV2Stub(channel, callOptions);
        }
      };
    return ChannelServiceBlockingV2Stub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static ChannelServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ChannelServiceBlockingStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ChannelServiceBlockingStub>() {
        @java.lang.Override
        public ChannelServiceBlockingStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ChannelServiceBlockingStub(channel, callOptions);
        }
      };
    return ChannelServiceBlockingStub.newStub(factory, channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static ChannelServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ChannelServiceFutureStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ChannelServiceFutureStub>() {
        @java.lang.Override
        public ChannelServiceFutureStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ChannelServiceFutureStub(channel, callOptions);
        }
      };
    return ChannelServiceFutureStub.newStub(factory, channel);
  }

  /**
   */
  public interface AsyncService {

    /**
     */
    default void postMessage(talon.v1.Api.PostChannelMessageRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.PostChannelMessageResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getPostMessageMethod(), responseObserver);
    }

    /**
     */
    default void getMessage(talon.v1.Api.GetChannelMessageRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.ChannelMessageResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetMessageMethod(), responseObserver);
    }

    /**
     */
    default void listMessages(talon.v1.Api.ListChannelMessagesRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.ListChannelMessagesResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListMessagesMethod(), responseObserver);
    }

    /**
     */
    default void streamEvents(talon.v1.Api.StreamChannelEventsRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.ChannelEvent> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getStreamEventsMethod(), responseObserver);
    }
  }

  /**
   * Base class for the server implementation of the service ChannelService.
   */
  public static abstract class ChannelServiceImplBase
      implements io.grpc.BindableService, AsyncService {

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return ChannelServiceGrpc.bindService(this);
    }
  }

  /**
   * A stub to allow clients to do asynchronous rpc calls to service ChannelService.
   */
  public static final class ChannelServiceStub
      extends io.grpc.stub.AbstractAsyncStub<ChannelServiceStub> {
    private ChannelServiceStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ChannelServiceStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ChannelServiceStub(channel, callOptions);
    }

    /**
     */
    public void postMessage(talon.v1.Api.PostChannelMessageRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.PostChannelMessageResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getPostMessageMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getMessage(talon.v1.Api.GetChannelMessageRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.ChannelMessageResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetMessageMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listMessages(talon.v1.Api.ListChannelMessagesRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.ListChannelMessagesResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListMessagesMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void streamEvents(talon.v1.Api.StreamChannelEventsRequest request,
        io.grpc.stub.StreamObserver<talon.events.Events.ChannelEvent> responseObserver) {
      io.grpc.stub.ClientCalls.asyncServerStreamingCall(
          getChannel().newCall(getStreamEventsMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   * A stub to allow clients to do synchronous rpc calls to service ChannelService.
   */
  public static final class ChannelServiceBlockingV2Stub
      extends io.grpc.stub.AbstractBlockingStub<ChannelServiceBlockingV2Stub> {
    private ChannelServiceBlockingV2Stub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ChannelServiceBlockingV2Stub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ChannelServiceBlockingV2Stub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Api.PostChannelMessageResponse postMessage(talon.v1.Api.PostChannelMessageRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getPostMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Api.ChannelMessageResponse getMessage(talon.v1.Api.GetChannelMessageRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Api.ListChannelMessagesResponse listMessages(talon.v1.Api.ListChannelMessagesRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListMessagesMethod(), getCallOptions(), request);
    }

    /**
     */
    @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/10918")
    public io.grpc.stub.BlockingClientCall<?, talon.events.Events.ChannelEvent>
        streamEvents(talon.v1.Api.StreamChannelEventsRequest request) {
      return io.grpc.stub.ClientCalls.blockingV2ServerStreamingCall(
          getChannel(), getStreamEventsMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do limited synchronous rpc calls to service ChannelService.
   */
  public static final class ChannelServiceBlockingStub
      extends io.grpc.stub.AbstractBlockingStub<ChannelServiceBlockingStub> {
    private ChannelServiceBlockingStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ChannelServiceBlockingStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ChannelServiceBlockingStub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Api.PostChannelMessageResponse postMessage(talon.v1.Api.PostChannelMessageRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getPostMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Api.ChannelMessageResponse getMessage(talon.v1.Api.GetChannelMessageRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetMessageMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Api.ListChannelMessagesResponse listMessages(talon.v1.Api.ListChannelMessagesRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListMessagesMethod(), getCallOptions(), request);
    }

    /**
     */
    public java.util.Iterator<talon.events.Events.ChannelEvent> streamEvents(
        talon.v1.Api.StreamChannelEventsRequest request) {
      return io.grpc.stub.ClientCalls.blockingServerStreamingCall(
          getChannel(), getStreamEventsMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do ListenableFuture-style rpc calls to service ChannelService.
   */
  public static final class ChannelServiceFutureStub
      extends io.grpc.stub.AbstractFutureStub<ChannelServiceFutureStub> {
    private ChannelServiceFutureStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ChannelServiceFutureStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ChannelServiceFutureStub(channel, callOptions);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Api.PostChannelMessageResponse> postMessage(
        talon.v1.Api.PostChannelMessageRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getPostMessageMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Api.ChannelMessageResponse> getMessage(
        talon.v1.Api.GetChannelMessageRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetMessageMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Api.ListChannelMessagesResponse> listMessages(
        talon.v1.Api.ListChannelMessagesRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListMessagesMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_POST_MESSAGE = 0;
  private static final int METHODID_GET_MESSAGE = 1;
  private static final int METHODID_LIST_MESSAGES = 2;
  private static final int METHODID_STREAM_EVENTS = 3;

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
        case METHODID_POST_MESSAGE:
          serviceImpl.postMessage((talon.v1.Api.PostChannelMessageRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Api.PostChannelMessageResponse>) responseObserver);
          break;
        case METHODID_GET_MESSAGE:
          serviceImpl.getMessage((talon.v1.Api.GetChannelMessageRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Api.ChannelMessageResponse>) responseObserver);
          break;
        case METHODID_LIST_MESSAGES:
          serviceImpl.listMessages((talon.v1.Api.ListChannelMessagesRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Api.ListChannelMessagesResponse>) responseObserver);
          break;
        case METHODID_STREAM_EVENTS:
          serviceImpl.streamEvents((talon.v1.Api.StreamChannelEventsRequest) request,
              (io.grpc.stub.StreamObserver<talon.events.Events.ChannelEvent>) responseObserver);
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
          getPostMessageMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Api.PostChannelMessageRequest,
              talon.v1.Api.PostChannelMessageResponse>(
                service, METHODID_POST_MESSAGE)))
        .addMethod(
          getGetMessageMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Api.GetChannelMessageRequest,
              talon.v1.Api.ChannelMessageResponse>(
                service, METHODID_GET_MESSAGE)))
        .addMethod(
          getListMessagesMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Api.ListChannelMessagesRequest,
              talon.v1.Api.ListChannelMessagesResponse>(
                service, METHODID_LIST_MESSAGES)))
        .addMethod(
          getStreamEventsMethod(),
          io.grpc.stub.ServerCalls.asyncServerStreamingCall(
            new MethodHandlers<
              talon.v1.Api.StreamChannelEventsRequest,
              talon.events.Events.ChannelEvent>(
                service, METHODID_STREAM_EVENTS)))
        .build();
  }

  private static abstract class ChannelServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    ChannelServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return talon.v1.Api.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("ChannelService");
    }
  }

  private static final class ChannelServiceFileDescriptorSupplier
      extends ChannelServiceBaseDescriptorSupplier {
    ChannelServiceFileDescriptorSupplier() {}
  }

  private static final class ChannelServiceMethodDescriptorSupplier
      extends ChannelServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final java.lang.String methodName;

    ChannelServiceMethodDescriptorSupplier(java.lang.String methodName) {
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
      synchronized (ChannelServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new ChannelServiceFileDescriptorSupplier())
              .addMethod(getPostMessageMethod())
              .addMethod(getGetMessageMethod())
              .addMethod(getListMessagesMethod())
              .addMethod(getStreamEventsMethod())
              .build();
        }
      }
    }
    return result;
  }
}
