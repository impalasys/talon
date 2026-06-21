package talon.v1;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class AuthServiceGrpc {

  private AuthServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.v1.AuthService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<talon.v1.Api.GetSsoConfigRequest,
      talon.v1.Api.GetSsoConfigResponse> getGetSsoConfigMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetSsoConfig",
      requestType = talon.v1.Api.GetSsoConfigRequest.class,
      responseType = talon.v1.Api.GetSsoConfigResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Api.GetSsoConfigRequest,
      talon.v1.Api.GetSsoConfigResponse> getGetSsoConfigMethod() {
    io.grpc.MethodDescriptor<talon.v1.Api.GetSsoConfigRequest, talon.v1.Api.GetSsoConfigResponse> getGetSsoConfigMethod;
    if ((getGetSsoConfigMethod = AuthServiceGrpc.getGetSsoConfigMethod) == null) {
      synchronized (AuthServiceGrpc.class) {
        if ((getGetSsoConfigMethod = AuthServiceGrpc.getGetSsoConfigMethod) == null) {
          AuthServiceGrpc.getGetSsoConfigMethod = getGetSsoConfigMethod =
              io.grpc.MethodDescriptor.<talon.v1.Api.GetSsoConfigRequest, talon.v1.Api.GetSsoConfigResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetSsoConfig"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.GetSsoConfigRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.GetSsoConfigResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AuthServiceMethodDescriptorSupplier("GetSsoConfig"))
              .build();
        }
      }
    }
    return getGetSsoConfigMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Api.ExchangeOidcTokenRequest,
      talon.v1.Api.ExchangeOidcTokenResponse> getExchangeOidcTokenMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ExchangeOidcToken",
      requestType = talon.v1.Api.ExchangeOidcTokenRequest.class,
      responseType = talon.v1.Api.ExchangeOidcTokenResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Api.ExchangeOidcTokenRequest,
      talon.v1.Api.ExchangeOidcTokenResponse> getExchangeOidcTokenMethod() {
    io.grpc.MethodDescriptor<talon.v1.Api.ExchangeOidcTokenRequest, talon.v1.Api.ExchangeOidcTokenResponse> getExchangeOidcTokenMethod;
    if ((getExchangeOidcTokenMethod = AuthServiceGrpc.getExchangeOidcTokenMethod) == null) {
      synchronized (AuthServiceGrpc.class) {
        if ((getExchangeOidcTokenMethod = AuthServiceGrpc.getExchangeOidcTokenMethod) == null) {
          AuthServiceGrpc.getExchangeOidcTokenMethod = getExchangeOidcTokenMethod =
              io.grpc.MethodDescriptor.<talon.v1.Api.ExchangeOidcTokenRequest, talon.v1.Api.ExchangeOidcTokenResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ExchangeOidcToken"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.ExchangeOidcTokenRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.ExchangeOidcTokenResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AuthServiceMethodDescriptorSupplier("ExchangeOidcToken"))
              .build();
        }
      }
    }
    return getExchangeOidcTokenMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static AuthServiceStub newStub(io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<AuthServiceStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<AuthServiceStub>() {
        @java.lang.Override
        public AuthServiceStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new AuthServiceStub(channel, callOptions);
        }
      };
    return AuthServiceStub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports all types of calls on the service
   */
  public static AuthServiceBlockingV2Stub newBlockingV2Stub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<AuthServiceBlockingV2Stub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<AuthServiceBlockingV2Stub>() {
        @java.lang.Override
        public AuthServiceBlockingV2Stub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new AuthServiceBlockingV2Stub(channel, callOptions);
        }
      };
    return AuthServiceBlockingV2Stub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static AuthServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<AuthServiceBlockingStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<AuthServiceBlockingStub>() {
        @java.lang.Override
        public AuthServiceBlockingStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new AuthServiceBlockingStub(channel, callOptions);
        }
      };
    return AuthServiceBlockingStub.newStub(factory, channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static AuthServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<AuthServiceFutureStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<AuthServiceFutureStub>() {
        @java.lang.Override
        public AuthServiceFutureStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new AuthServiceFutureStub(channel, callOptions);
        }
      };
    return AuthServiceFutureStub.newStub(factory, channel);
  }

  /**
   */
  public interface AsyncService {

    /**
     */
    default void getSsoConfig(talon.v1.Api.GetSsoConfigRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.GetSsoConfigResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetSsoConfigMethod(), responseObserver);
    }

    /**
     */
    default void exchangeOidcToken(talon.v1.Api.ExchangeOidcTokenRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.ExchangeOidcTokenResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getExchangeOidcTokenMethod(), responseObserver);
    }
  }

  /**
   * Base class for the server implementation of the service AuthService.
   */
  public static abstract class AuthServiceImplBase
      implements io.grpc.BindableService, AsyncService {

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return AuthServiceGrpc.bindService(this);
    }
  }

  /**
   * A stub to allow clients to do asynchronous rpc calls to service AuthService.
   */
  public static final class AuthServiceStub
      extends io.grpc.stub.AbstractAsyncStub<AuthServiceStub> {
    private AuthServiceStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected AuthServiceStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new AuthServiceStub(channel, callOptions);
    }

    /**
     */
    public void getSsoConfig(talon.v1.Api.GetSsoConfigRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.GetSsoConfigResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetSsoConfigMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void exchangeOidcToken(talon.v1.Api.ExchangeOidcTokenRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.ExchangeOidcTokenResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getExchangeOidcTokenMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   * A stub to allow clients to do synchronous rpc calls to service AuthService.
   */
  public static final class AuthServiceBlockingV2Stub
      extends io.grpc.stub.AbstractBlockingStub<AuthServiceBlockingV2Stub> {
    private AuthServiceBlockingV2Stub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected AuthServiceBlockingV2Stub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new AuthServiceBlockingV2Stub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Api.GetSsoConfigResponse getSsoConfig(talon.v1.Api.GetSsoConfigRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetSsoConfigMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Api.ExchangeOidcTokenResponse exchangeOidcToken(talon.v1.Api.ExchangeOidcTokenRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getExchangeOidcTokenMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do limited synchronous rpc calls to service AuthService.
   */
  public static final class AuthServiceBlockingStub
      extends io.grpc.stub.AbstractBlockingStub<AuthServiceBlockingStub> {
    private AuthServiceBlockingStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected AuthServiceBlockingStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new AuthServiceBlockingStub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Api.GetSsoConfigResponse getSsoConfig(talon.v1.Api.GetSsoConfigRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetSsoConfigMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Api.ExchangeOidcTokenResponse exchangeOidcToken(talon.v1.Api.ExchangeOidcTokenRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getExchangeOidcTokenMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do ListenableFuture-style rpc calls to service AuthService.
   */
  public static final class AuthServiceFutureStub
      extends io.grpc.stub.AbstractFutureStub<AuthServiceFutureStub> {
    private AuthServiceFutureStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected AuthServiceFutureStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new AuthServiceFutureStub(channel, callOptions);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Api.GetSsoConfigResponse> getSsoConfig(
        talon.v1.Api.GetSsoConfigRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetSsoConfigMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Api.ExchangeOidcTokenResponse> exchangeOidcToken(
        talon.v1.Api.ExchangeOidcTokenRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getExchangeOidcTokenMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_GET_SSO_CONFIG = 0;
  private static final int METHODID_EXCHANGE_OIDC_TOKEN = 1;

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
        case METHODID_GET_SSO_CONFIG:
          serviceImpl.getSsoConfig((talon.v1.Api.GetSsoConfigRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Api.GetSsoConfigResponse>) responseObserver);
          break;
        case METHODID_EXCHANGE_OIDC_TOKEN:
          serviceImpl.exchangeOidcToken((talon.v1.Api.ExchangeOidcTokenRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Api.ExchangeOidcTokenResponse>) responseObserver);
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
          getGetSsoConfigMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Api.GetSsoConfigRequest,
              talon.v1.Api.GetSsoConfigResponse>(
                service, METHODID_GET_SSO_CONFIG)))
        .addMethod(
          getExchangeOidcTokenMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Api.ExchangeOidcTokenRequest,
              talon.v1.Api.ExchangeOidcTokenResponse>(
                service, METHODID_EXCHANGE_OIDC_TOKEN)))
        .build();
  }

  private static abstract class AuthServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    AuthServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return talon.v1.Api.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("AuthService");
    }
  }

  private static final class AuthServiceFileDescriptorSupplier
      extends AuthServiceBaseDescriptorSupplier {
    AuthServiceFileDescriptorSupplier() {}
  }

  private static final class AuthServiceMethodDescriptorSupplier
      extends AuthServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final java.lang.String methodName;

    AuthServiceMethodDescriptorSupplier(java.lang.String methodName) {
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
      synchronized (AuthServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new AuthServiceFileDescriptorSupplier())
              .addMethod(getGetSsoConfigMethod())
              .addMethod(getExchangeOidcTokenMethod())
              .build();
        }
      }
    }
    return result;
  }
}
