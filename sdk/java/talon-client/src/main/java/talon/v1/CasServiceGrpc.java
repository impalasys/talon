package talon.v1;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class CasServiceGrpc {

  private CasServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.v1.CasService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<talon.v1.Cas.GetCasObjectRequest,
      talon.v1.Cas.GetCasObjectResponse> getGetObjectMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetObject",
      requestType = talon.v1.Cas.GetCasObjectRequest.class,
      responseType = talon.v1.Cas.GetCasObjectResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Cas.GetCasObjectRequest,
      talon.v1.Cas.GetCasObjectResponse> getGetObjectMethod() {
    io.grpc.MethodDescriptor<talon.v1.Cas.GetCasObjectRequest, talon.v1.Cas.GetCasObjectResponse> getGetObjectMethod;
    if ((getGetObjectMethod = CasServiceGrpc.getGetObjectMethod) == null) {
      synchronized (CasServiceGrpc.class) {
        if ((getGetObjectMethod = CasServiceGrpc.getGetObjectMethod) == null) {
          CasServiceGrpc.getGetObjectMethod = getGetObjectMethod =
              io.grpc.MethodDescriptor.<talon.v1.Cas.GetCasObjectRequest, talon.v1.Cas.GetCasObjectResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetObject"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Cas.GetCasObjectRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Cas.GetCasObjectResponse.getDefaultInstance()))
              .setSchemaDescriptor(new CasServiceMethodDescriptorSupplier("GetObject"))
              .build();
        }
      }
    }
    return getGetObjectMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static CasServiceStub newStub(io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<CasServiceStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<CasServiceStub>() {
        @java.lang.Override
        public CasServiceStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new CasServiceStub(channel, callOptions);
        }
      };
    return CasServiceStub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports all types of calls on the service
   */
  public static CasServiceBlockingV2Stub newBlockingV2Stub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<CasServiceBlockingV2Stub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<CasServiceBlockingV2Stub>() {
        @java.lang.Override
        public CasServiceBlockingV2Stub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new CasServiceBlockingV2Stub(channel, callOptions);
        }
      };
    return CasServiceBlockingV2Stub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static CasServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<CasServiceBlockingStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<CasServiceBlockingStub>() {
        @java.lang.Override
        public CasServiceBlockingStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new CasServiceBlockingStub(channel, callOptions);
        }
      };
    return CasServiceBlockingStub.newStub(factory, channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static CasServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<CasServiceFutureStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<CasServiceFutureStub>() {
        @java.lang.Override
        public CasServiceFutureStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new CasServiceFutureStub(channel, callOptions);
        }
      };
    return CasServiceFutureStub.newStub(factory, channel);
  }

  /**
   */
  public interface AsyncService {

    /**
     */
    default void getObject(talon.v1.Cas.GetCasObjectRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Cas.GetCasObjectResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetObjectMethod(), responseObserver);
    }
  }

  /**
   * Base class for the server implementation of the service CasService.
   */
  public static abstract class CasServiceImplBase
      implements io.grpc.BindableService, AsyncService {

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return CasServiceGrpc.bindService(this);
    }
  }

  /**
   * A stub to allow clients to do asynchronous rpc calls to service CasService.
   */
  public static final class CasServiceStub
      extends io.grpc.stub.AbstractAsyncStub<CasServiceStub> {
    private CasServiceStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected CasServiceStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new CasServiceStub(channel, callOptions);
    }

    /**
     */
    public void getObject(talon.v1.Cas.GetCasObjectRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Cas.GetCasObjectResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetObjectMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   * A stub to allow clients to do synchronous rpc calls to service CasService.
   */
  public static final class CasServiceBlockingV2Stub
      extends io.grpc.stub.AbstractBlockingStub<CasServiceBlockingV2Stub> {
    private CasServiceBlockingV2Stub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected CasServiceBlockingV2Stub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new CasServiceBlockingV2Stub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Cas.GetCasObjectResponse getObject(talon.v1.Cas.GetCasObjectRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetObjectMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do limited synchronous rpc calls to service CasService.
   */
  public static final class CasServiceBlockingStub
      extends io.grpc.stub.AbstractBlockingStub<CasServiceBlockingStub> {
    private CasServiceBlockingStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected CasServiceBlockingStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new CasServiceBlockingStub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Cas.GetCasObjectResponse getObject(talon.v1.Cas.GetCasObjectRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetObjectMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do ListenableFuture-style rpc calls to service CasService.
   */
  public static final class CasServiceFutureStub
      extends io.grpc.stub.AbstractFutureStub<CasServiceFutureStub> {
    private CasServiceFutureStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected CasServiceFutureStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new CasServiceFutureStub(channel, callOptions);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Cas.GetCasObjectResponse> getObject(
        talon.v1.Cas.GetCasObjectRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetObjectMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_GET_OBJECT = 0;

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
        case METHODID_GET_OBJECT:
          serviceImpl.getObject((talon.v1.Cas.GetCasObjectRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Cas.GetCasObjectResponse>) responseObserver);
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
          getGetObjectMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Cas.GetCasObjectRequest,
              talon.v1.Cas.GetCasObjectResponse>(
                service, METHODID_GET_OBJECT)))
        .build();
  }

  private static abstract class CasServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    CasServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return talon.v1.Cas.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("CasService");
    }
  }

  private static final class CasServiceFileDescriptorSupplier
      extends CasServiceBaseDescriptorSupplier {
    CasServiceFileDescriptorSupplier() {}
  }

  private static final class CasServiceMethodDescriptorSupplier
      extends CasServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final java.lang.String methodName;

    CasServiceMethodDescriptorSupplier(java.lang.String methodName) {
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
      synchronized (CasServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new CasServiceFileDescriptorSupplier())
              .addMethod(getGetObjectMethod())
              .build();
        }
      }
    }
    return result;
  }
}
