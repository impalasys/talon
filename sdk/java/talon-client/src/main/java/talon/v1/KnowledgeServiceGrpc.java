package talon.v1;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class KnowledgeServiceGrpc {

  private KnowledgeServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.v1.KnowledgeService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<talon.v1.Knowledge.GetKnowledgeRequest,
      talon.v1.Knowledge.KnowledgeResponse> getGetMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Get",
      requestType = talon.v1.Knowledge.GetKnowledgeRequest.class,
      responseType = talon.v1.Knowledge.KnowledgeResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Knowledge.GetKnowledgeRequest,
      talon.v1.Knowledge.KnowledgeResponse> getGetMethod() {
    io.grpc.MethodDescriptor<talon.v1.Knowledge.GetKnowledgeRequest, talon.v1.Knowledge.KnowledgeResponse> getGetMethod;
    if ((getGetMethod = KnowledgeServiceGrpc.getGetMethod) == null) {
      synchronized (KnowledgeServiceGrpc.class) {
        if ((getGetMethod = KnowledgeServiceGrpc.getGetMethod) == null) {
          KnowledgeServiceGrpc.getGetMethod = getGetMethod =
              io.grpc.MethodDescriptor.<talon.v1.Knowledge.GetKnowledgeRequest, talon.v1.Knowledge.KnowledgeResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Get"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Knowledge.GetKnowledgeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Knowledge.KnowledgeResponse.getDefaultInstance()))
              .setSchemaDescriptor(new KnowledgeServiceMethodDescriptorSupplier("Get"))
              .build();
        }
      }
    }
    return getGetMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Knowledge.SearchKnowledgeRequest,
      talon.v1.Knowledge.SearchKnowledgeResponse> getSearchMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Search",
      requestType = talon.v1.Knowledge.SearchKnowledgeRequest.class,
      responseType = talon.v1.Knowledge.SearchKnowledgeResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Knowledge.SearchKnowledgeRequest,
      talon.v1.Knowledge.SearchKnowledgeResponse> getSearchMethod() {
    io.grpc.MethodDescriptor<talon.v1.Knowledge.SearchKnowledgeRequest, talon.v1.Knowledge.SearchKnowledgeResponse> getSearchMethod;
    if ((getSearchMethod = KnowledgeServiceGrpc.getSearchMethod) == null) {
      synchronized (KnowledgeServiceGrpc.class) {
        if ((getSearchMethod = KnowledgeServiceGrpc.getSearchMethod) == null) {
          KnowledgeServiceGrpc.getSearchMethod = getSearchMethod =
              io.grpc.MethodDescriptor.<talon.v1.Knowledge.SearchKnowledgeRequest, talon.v1.Knowledge.SearchKnowledgeResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Search"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Knowledge.SearchKnowledgeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Knowledge.SearchKnowledgeResponse.getDefaultInstance()))
              .setSchemaDescriptor(new KnowledgeServiceMethodDescriptorSupplier("Search"))
              .build();
        }
      }
    }
    return getSearchMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static KnowledgeServiceStub newStub(io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<KnowledgeServiceStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<KnowledgeServiceStub>() {
        @java.lang.Override
        public KnowledgeServiceStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new KnowledgeServiceStub(channel, callOptions);
        }
      };
    return KnowledgeServiceStub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports all types of calls on the service
   */
  public static KnowledgeServiceBlockingV2Stub newBlockingV2Stub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<KnowledgeServiceBlockingV2Stub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<KnowledgeServiceBlockingV2Stub>() {
        @java.lang.Override
        public KnowledgeServiceBlockingV2Stub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new KnowledgeServiceBlockingV2Stub(channel, callOptions);
        }
      };
    return KnowledgeServiceBlockingV2Stub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static KnowledgeServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<KnowledgeServiceBlockingStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<KnowledgeServiceBlockingStub>() {
        @java.lang.Override
        public KnowledgeServiceBlockingStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new KnowledgeServiceBlockingStub(channel, callOptions);
        }
      };
    return KnowledgeServiceBlockingStub.newStub(factory, channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static KnowledgeServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<KnowledgeServiceFutureStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<KnowledgeServiceFutureStub>() {
        @java.lang.Override
        public KnowledgeServiceFutureStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new KnowledgeServiceFutureStub(channel, callOptions);
        }
      };
    return KnowledgeServiceFutureStub.newStub(factory, channel);
  }

  /**
   */
  public interface AsyncService {

    /**
     */
    default void get(talon.v1.Knowledge.GetKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Knowledge.KnowledgeResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetMethod(), responseObserver);
    }

    /**
     */
    default void search(talon.v1.Knowledge.SearchKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Knowledge.SearchKnowledgeResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getSearchMethod(), responseObserver);
    }
  }

  /**
   * Base class for the server implementation of the service KnowledgeService.
   */
  public static abstract class KnowledgeServiceImplBase
      implements io.grpc.BindableService, AsyncService {

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return KnowledgeServiceGrpc.bindService(this);
    }
  }

  /**
   * A stub to allow clients to do asynchronous rpc calls to service KnowledgeService.
   */
  public static final class KnowledgeServiceStub
      extends io.grpc.stub.AbstractAsyncStub<KnowledgeServiceStub> {
    private KnowledgeServiceStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected KnowledgeServiceStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new KnowledgeServiceStub(channel, callOptions);
    }

    /**
     */
    public void get(talon.v1.Knowledge.GetKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Knowledge.KnowledgeResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void search(talon.v1.Knowledge.SearchKnowledgeRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Knowledge.SearchKnowledgeResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getSearchMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   * A stub to allow clients to do synchronous rpc calls to service KnowledgeService.
   */
  public static final class KnowledgeServiceBlockingV2Stub
      extends io.grpc.stub.AbstractBlockingStub<KnowledgeServiceBlockingV2Stub> {
    private KnowledgeServiceBlockingV2Stub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected KnowledgeServiceBlockingV2Stub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new KnowledgeServiceBlockingV2Stub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Knowledge.KnowledgeResponse get(talon.v1.Knowledge.GetKnowledgeRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Knowledge.SearchKnowledgeResponse search(talon.v1.Knowledge.SearchKnowledgeRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getSearchMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do limited synchronous rpc calls to service KnowledgeService.
   */
  public static final class KnowledgeServiceBlockingStub
      extends io.grpc.stub.AbstractBlockingStub<KnowledgeServiceBlockingStub> {
    private KnowledgeServiceBlockingStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected KnowledgeServiceBlockingStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new KnowledgeServiceBlockingStub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Knowledge.KnowledgeResponse get(talon.v1.Knowledge.GetKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Knowledge.SearchKnowledgeResponse search(talon.v1.Knowledge.SearchKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getSearchMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do ListenableFuture-style rpc calls to service KnowledgeService.
   */
  public static final class KnowledgeServiceFutureStub
      extends io.grpc.stub.AbstractFutureStub<KnowledgeServiceFutureStub> {
    private KnowledgeServiceFutureStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected KnowledgeServiceFutureStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new KnowledgeServiceFutureStub(channel, callOptions);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Knowledge.KnowledgeResponse> get(
        talon.v1.Knowledge.GetKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Knowledge.SearchKnowledgeResponse> search(
        talon.v1.Knowledge.SearchKnowledgeRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getSearchMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_GET = 0;
  private static final int METHODID_SEARCH = 1;

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
        case METHODID_GET:
          serviceImpl.get((talon.v1.Knowledge.GetKnowledgeRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Knowledge.KnowledgeResponse>) responseObserver);
          break;
        case METHODID_SEARCH:
          serviceImpl.search((talon.v1.Knowledge.SearchKnowledgeRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Knowledge.SearchKnowledgeResponse>) responseObserver);
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
          getGetMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Knowledge.GetKnowledgeRequest,
              talon.v1.Knowledge.KnowledgeResponse>(
                service, METHODID_GET)))
        .addMethod(
          getSearchMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Knowledge.SearchKnowledgeRequest,
              talon.v1.Knowledge.SearchKnowledgeResponse>(
                service, METHODID_SEARCH)))
        .build();
  }

  private static abstract class KnowledgeServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    KnowledgeServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return talon.v1.Knowledge.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("KnowledgeService");
    }
  }

  private static final class KnowledgeServiceFileDescriptorSupplier
      extends KnowledgeServiceBaseDescriptorSupplier {
    KnowledgeServiceFileDescriptorSupplier() {}
  }

  private static final class KnowledgeServiceMethodDescriptorSupplier
      extends KnowledgeServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final java.lang.String methodName;

    KnowledgeServiceMethodDescriptorSupplier(java.lang.String methodName) {
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
      synchronized (KnowledgeServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new KnowledgeServiceFileDescriptorSupplier())
              .addMethod(getGetMethod())
              .addMethod(getSearchMethod())
              .build();
        }
      }
    }
    return result;
  }
}
