package talon.v1;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class AuthServiceGrpc {

  private AuthServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.v1.AuthService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<talon.v1.Auth.GetSsoConfigRequest,
      talon.v1.Auth.GetSsoConfigResponse> getGetSsoConfigMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetSsoConfig",
      requestType = talon.v1.Auth.GetSsoConfigRequest.class,
      responseType = talon.v1.Auth.GetSsoConfigResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Auth.GetSsoConfigRequest,
      talon.v1.Auth.GetSsoConfigResponse> getGetSsoConfigMethod() {
    io.grpc.MethodDescriptor<talon.v1.Auth.GetSsoConfigRequest, talon.v1.Auth.GetSsoConfigResponse> getGetSsoConfigMethod;
    if ((getGetSsoConfigMethod = AuthServiceGrpc.getGetSsoConfigMethod) == null) {
      synchronized (AuthServiceGrpc.class) {
        if ((getGetSsoConfigMethod = AuthServiceGrpc.getGetSsoConfigMethod) == null) {
          AuthServiceGrpc.getGetSsoConfigMethod = getGetSsoConfigMethod =
              io.grpc.MethodDescriptor.<talon.v1.Auth.GetSsoConfigRequest, talon.v1.Auth.GetSsoConfigResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetSsoConfig"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.GetSsoConfigRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.GetSsoConfigResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AuthServiceMethodDescriptorSupplier("GetSsoConfig"))
              .build();
        }
      }
    }
    return getGetSsoConfigMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Auth.ExchangeOidcTokenRequest,
      talon.v1.Auth.ExchangeOidcTokenResponse> getExchangeOidcTokenMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ExchangeOidcToken",
      requestType = talon.v1.Auth.ExchangeOidcTokenRequest.class,
      responseType = talon.v1.Auth.ExchangeOidcTokenResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Auth.ExchangeOidcTokenRequest,
      talon.v1.Auth.ExchangeOidcTokenResponse> getExchangeOidcTokenMethod() {
    io.grpc.MethodDescriptor<talon.v1.Auth.ExchangeOidcTokenRequest, talon.v1.Auth.ExchangeOidcTokenResponse> getExchangeOidcTokenMethod;
    if ((getExchangeOidcTokenMethod = AuthServiceGrpc.getExchangeOidcTokenMethod) == null) {
      synchronized (AuthServiceGrpc.class) {
        if ((getExchangeOidcTokenMethod = AuthServiceGrpc.getExchangeOidcTokenMethod) == null) {
          AuthServiceGrpc.getExchangeOidcTokenMethod = getExchangeOidcTokenMethod =
              io.grpc.MethodDescriptor.<talon.v1.Auth.ExchangeOidcTokenRequest, talon.v1.Auth.ExchangeOidcTokenResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ExchangeOidcToken"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.ExchangeOidcTokenRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.ExchangeOidcTokenResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AuthServiceMethodDescriptorSupplier("ExchangeOidcToken"))
              .build();
        }
      }
    }
    return getExchangeOidcTokenMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Auth.MintAccessTokenRequest,
      talon.v1.Auth.MintAccessTokenResponse> getMintAccessTokenMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "MintAccessToken",
      requestType = talon.v1.Auth.MintAccessTokenRequest.class,
      responseType = talon.v1.Auth.MintAccessTokenResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Auth.MintAccessTokenRequest,
      talon.v1.Auth.MintAccessTokenResponse> getMintAccessTokenMethod() {
    io.grpc.MethodDescriptor<talon.v1.Auth.MintAccessTokenRequest, talon.v1.Auth.MintAccessTokenResponse> getMintAccessTokenMethod;
    if ((getMintAccessTokenMethod = AuthServiceGrpc.getMintAccessTokenMethod) == null) {
      synchronized (AuthServiceGrpc.class) {
        if ((getMintAccessTokenMethod = AuthServiceGrpc.getMintAccessTokenMethod) == null) {
          AuthServiceGrpc.getMintAccessTokenMethod = getMintAccessTokenMethod =
              io.grpc.MethodDescriptor.<talon.v1.Auth.MintAccessTokenRequest, talon.v1.Auth.MintAccessTokenResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "MintAccessToken"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.MintAccessTokenRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.MintAccessTokenResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AuthServiceMethodDescriptorSupplier("MintAccessToken"))
              .build();
        }
      }
    }
    return getMintAccessTokenMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Auth.CreateApiKeyRequest,
      talon.v1.Auth.CreateApiKeyResponse> getCreateApiKeyMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateApiKey",
      requestType = talon.v1.Auth.CreateApiKeyRequest.class,
      responseType = talon.v1.Auth.CreateApiKeyResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Auth.CreateApiKeyRequest,
      talon.v1.Auth.CreateApiKeyResponse> getCreateApiKeyMethod() {
    io.grpc.MethodDescriptor<talon.v1.Auth.CreateApiKeyRequest, talon.v1.Auth.CreateApiKeyResponse> getCreateApiKeyMethod;
    if ((getCreateApiKeyMethod = AuthServiceGrpc.getCreateApiKeyMethod) == null) {
      synchronized (AuthServiceGrpc.class) {
        if ((getCreateApiKeyMethod = AuthServiceGrpc.getCreateApiKeyMethod) == null) {
          AuthServiceGrpc.getCreateApiKeyMethod = getCreateApiKeyMethod =
              io.grpc.MethodDescriptor.<talon.v1.Auth.CreateApiKeyRequest, talon.v1.Auth.CreateApiKeyResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateApiKey"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.CreateApiKeyRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.CreateApiKeyResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AuthServiceMethodDescriptorSupplier("CreateApiKey"))
              .build();
        }
      }
    }
    return getCreateApiKeyMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Auth.ListApiKeysRequest,
      talon.v1.Auth.ListApiKeysResponse> getListApiKeysMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListApiKeys",
      requestType = talon.v1.Auth.ListApiKeysRequest.class,
      responseType = talon.v1.Auth.ListApiKeysResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Auth.ListApiKeysRequest,
      talon.v1.Auth.ListApiKeysResponse> getListApiKeysMethod() {
    io.grpc.MethodDescriptor<talon.v1.Auth.ListApiKeysRequest, talon.v1.Auth.ListApiKeysResponse> getListApiKeysMethod;
    if ((getListApiKeysMethod = AuthServiceGrpc.getListApiKeysMethod) == null) {
      synchronized (AuthServiceGrpc.class) {
        if ((getListApiKeysMethod = AuthServiceGrpc.getListApiKeysMethod) == null) {
          AuthServiceGrpc.getListApiKeysMethod = getListApiKeysMethod =
              io.grpc.MethodDescriptor.<talon.v1.Auth.ListApiKeysRequest, talon.v1.Auth.ListApiKeysResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListApiKeys"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.ListApiKeysRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.ListApiKeysResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AuthServiceMethodDescriptorSupplier("ListApiKeys"))
              .build();
        }
      }
    }
    return getListApiKeysMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Auth.RevokeApiKeyRequest,
      talon.v1.Auth.RevokeApiKeyResponse> getRevokeApiKeyMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "RevokeApiKey",
      requestType = talon.v1.Auth.RevokeApiKeyRequest.class,
      responseType = talon.v1.Auth.RevokeApiKeyResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Auth.RevokeApiKeyRequest,
      talon.v1.Auth.RevokeApiKeyResponse> getRevokeApiKeyMethod() {
    io.grpc.MethodDescriptor<talon.v1.Auth.RevokeApiKeyRequest, talon.v1.Auth.RevokeApiKeyResponse> getRevokeApiKeyMethod;
    if ((getRevokeApiKeyMethod = AuthServiceGrpc.getRevokeApiKeyMethod) == null) {
      synchronized (AuthServiceGrpc.class) {
        if ((getRevokeApiKeyMethod = AuthServiceGrpc.getRevokeApiKeyMethod) == null) {
          AuthServiceGrpc.getRevokeApiKeyMethod = getRevokeApiKeyMethod =
              io.grpc.MethodDescriptor.<talon.v1.Auth.RevokeApiKeyRequest, talon.v1.Auth.RevokeApiKeyResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "RevokeApiKey"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.RevokeApiKeyRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.RevokeApiKeyResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AuthServiceMethodDescriptorSupplier("RevokeApiKey"))
              .build();
        }
      }
    }
    return getRevokeApiKeyMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Auth.ExchangeApiKeyRequest,
      talon.v1.Auth.ExchangeApiKeyResponse> getExchangeApiKeyMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ExchangeApiKey",
      requestType = talon.v1.Auth.ExchangeApiKeyRequest.class,
      responseType = talon.v1.Auth.ExchangeApiKeyResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Auth.ExchangeApiKeyRequest,
      talon.v1.Auth.ExchangeApiKeyResponse> getExchangeApiKeyMethod() {
    io.grpc.MethodDescriptor<talon.v1.Auth.ExchangeApiKeyRequest, talon.v1.Auth.ExchangeApiKeyResponse> getExchangeApiKeyMethod;
    if ((getExchangeApiKeyMethod = AuthServiceGrpc.getExchangeApiKeyMethod) == null) {
      synchronized (AuthServiceGrpc.class) {
        if ((getExchangeApiKeyMethod = AuthServiceGrpc.getExchangeApiKeyMethod) == null) {
          AuthServiceGrpc.getExchangeApiKeyMethod = getExchangeApiKeyMethod =
              io.grpc.MethodDescriptor.<talon.v1.Auth.ExchangeApiKeyRequest, talon.v1.Auth.ExchangeApiKeyResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ExchangeApiKey"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.ExchangeApiKeyRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Auth.ExchangeApiKeyResponse.getDefaultInstance()))
              .setSchemaDescriptor(new AuthServiceMethodDescriptorSupplier("ExchangeApiKey"))
              .build();
        }
      }
    }
    return getExchangeApiKeyMethod;
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
    default void getSsoConfig(talon.v1.Auth.GetSsoConfigRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.GetSsoConfigResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetSsoConfigMethod(), responseObserver);
    }

    /**
     */
    default void exchangeOidcToken(talon.v1.Auth.ExchangeOidcTokenRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.ExchangeOidcTokenResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getExchangeOidcTokenMethod(), responseObserver);
    }

    /**
     */
    default void mintAccessToken(talon.v1.Auth.MintAccessTokenRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.MintAccessTokenResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getMintAccessTokenMethod(), responseObserver);
    }

    /**
     */
    default void createApiKey(talon.v1.Auth.CreateApiKeyRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.CreateApiKeyResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateApiKeyMethod(), responseObserver);
    }

    /**
     */
    default void listApiKeys(talon.v1.Auth.ListApiKeysRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.ListApiKeysResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListApiKeysMethod(), responseObserver);
    }

    /**
     */
    default void revokeApiKey(talon.v1.Auth.RevokeApiKeyRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.RevokeApiKeyResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getRevokeApiKeyMethod(), responseObserver);
    }

    /**
     */
    default void exchangeApiKey(talon.v1.Auth.ExchangeApiKeyRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.ExchangeApiKeyResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getExchangeApiKeyMethod(), responseObserver);
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
    public void getSsoConfig(talon.v1.Auth.GetSsoConfigRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.GetSsoConfigResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetSsoConfigMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void exchangeOidcToken(talon.v1.Auth.ExchangeOidcTokenRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.ExchangeOidcTokenResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getExchangeOidcTokenMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void mintAccessToken(talon.v1.Auth.MintAccessTokenRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.MintAccessTokenResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getMintAccessTokenMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void createApiKey(talon.v1.Auth.CreateApiKeyRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.CreateApiKeyResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateApiKeyMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listApiKeys(talon.v1.Auth.ListApiKeysRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.ListApiKeysResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListApiKeysMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void revokeApiKey(talon.v1.Auth.RevokeApiKeyRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.RevokeApiKeyResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getRevokeApiKeyMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void exchangeApiKey(talon.v1.Auth.ExchangeApiKeyRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Auth.ExchangeApiKeyResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getExchangeApiKeyMethod(), getCallOptions()), request, responseObserver);
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
    public talon.v1.Auth.GetSsoConfigResponse getSsoConfig(talon.v1.Auth.GetSsoConfigRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetSsoConfigMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Auth.ExchangeOidcTokenResponse exchangeOidcToken(talon.v1.Auth.ExchangeOidcTokenRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getExchangeOidcTokenMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Auth.MintAccessTokenResponse mintAccessToken(talon.v1.Auth.MintAccessTokenRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getMintAccessTokenMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Auth.CreateApiKeyResponse createApiKey(talon.v1.Auth.CreateApiKeyRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateApiKeyMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Auth.ListApiKeysResponse listApiKeys(talon.v1.Auth.ListApiKeysRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListApiKeysMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Auth.RevokeApiKeyResponse revokeApiKey(talon.v1.Auth.RevokeApiKeyRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getRevokeApiKeyMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Auth.ExchangeApiKeyResponse exchangeApiKey(talon.v1.Auth.ExchangeApiKeyRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getExchangeApiKeyMethod(), getCallOptions(), request);
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
    public talon.v1.Auth.GetSsoConfigResponse getSsoConfig(talon.v1.Auth.GetSsoConfigRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetSsoConfigMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Auth.ExchangeOidcTokenResponse exchangeOidcToken(talon.v1.Auth.ExchangeOidcTokenRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getExchangeOidcTokenMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Auth.MintAccessTokenResponse mintAccessToken(talon.v1.Auth.MintAccessTokenRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getMintAccessTokenMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Auth.CreateApiKeyResponse createApiKey(talon.v1.Auth.CreateApiKeyRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateApiKeyMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Auth.ListApiKeysResponse listApiKeys(talon.v1.Auth.ListApiKeysRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListApiKeysMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Auth.RevokeApiKeyResponse revokeApiKey(talon.v1.Auth.RevokeApiKeyRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getRevokeApiKeyMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Auth.ExchangeApiKeyResponse exchangeApiKey(talon.v1.Auth.ExchangeApiKeyRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getExchangeApiKeyMethod(), getCallOptions(), request);
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
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Auth.GetSsoConfigResponse> getSsoConfig(
        talon.v1.Auth.GetSsoConfigRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetSsoConfigMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Auth.ExchangeOidcTokenResponse> exchangeOidcToken(
        talon.v1.Auth.ExchangeOidcTokenRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getExchangeOidcTokenMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Auth.MintAccessTokenResponse> mintAccessToken(
        talon.v1.Auth.MintAccessTokenRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getMintAccessTokenMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Auth.CreateApiKeyResponse> createApiKey(
        talon.v1.Auth.CreateApiKeyRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateApiKeyMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Auth.ListApiKeysResponse> listApiKeys(
        talon.v1.Auth.ListApiKeysRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListApiKeysMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Auth.RevokeApiKeyResponse> revokeApiKey(
        talon.v1.Auth.RevokeApiKeyRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getRevokeApiKeyMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Auth.ExchangeApiKeyResponse> exchangeApiKey(
        talon.v1.Auth.ExchangeApiKeyRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getExchangeApiKeyMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_GET_SSO_CONFIG = 0;
  private static final int METHODID_EXCHANGE_OIDC_TOKEN = 1;
  private static final int METHODID_MINT_ACCESS_TOKEN = 2;
  private static final int METHODID_CREATE_API_KEY = 3;
  private static final int METHODID_LIST_API_KEYS = 4;
  private static final int METHODID_REVOKE_API_KEY = 5;
  private static final int METHODID_EXCHANGE_API_KEY = 6;

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
          serviceImpl.getSsoConfig((talon.v1.Auth.GetSsoConfigRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Auth.GetSsoConfigResponse>) responseObserver);
          break;
        case METHODID_EXCHANGE_OIDC_TOKEN:
          serviceImpl.exchangeOidcToken((talon.v1.Auth.ExchangeOidcTokenRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Auth.ExchangeOidcTokenResponse>) responseObserver);
          break;
        case METHODID_MINT_ACCESS_TOKEN:
          serviceImpl.mintAccessToken((talon.v1.Auth.MintAccessTokenRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Auth.MintAccessTokenResponse>) responseObserver);
          break;
        case METHODID_CREATE_API_KEY:
          serviceImpl.createApiKey((talon.v1.Auth.CreateApiKeyRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Auth.CreateApiKeyResponse>) responseObserver);
          break;
        case METHODID_LIST_API_KEYS:
          serviceImpl.listApiKeys((talon.v1.Auth.ListApiKeysRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Auth.ListApiKeysResponse>) responseObserver);
          break;
        case METHODID_REVOKE_API_KEY:
          serviceImpl.revokeApiKey((talon.v1.Auth.RevokeApiKeyRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Auth.RevokeApiKeyResponse>) responseObserver);
          break;
        case METHODID_EXCHANGE_API_KEY:
          serviceImpl.exchangeApiKey((talon.v1.Auth.ExchangeApiKeyRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Auth.ExchangeApiKeyResponse>) responseObserver);
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
              talon.v1.Auth.GetSsoConfigRequest,
              talon.v1.Auth.GetSsoConfigResponse>(
                service, METHODID_GET_SSO_CONFIG)))
        .addMethod(
          getExchangeOidcTokenMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Auth.ExchangeOidcTokenRequest,
              talon.v1.Auth.ExchangeOidcTokenResponse>(
                service, METHODID_EXCHANGE_OIDC_TOKEN)))
        .addMethod(
          getMintAccessTokenMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Auth.MintAccessTokenRequest,
              talon.v1.Auth.MintAccessTokenResponse>(
                service, METHODID_MINT_ACCESS_TOKEN)))
        .addMethod(
          getCreateApiKeyMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Auth.CreateApiKeyRequest,
              talon.v1.Auth.CreateApiKeyResponse>(
                service, METHODID_CREATE_API_KEY)))
        .addMethod(
          getListApiKeysMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Auth.ListApiKeysRequest,
              talon.v1.Auth.ListApiKeysResponse>(
                service, METHODID_LIST_API_KEYS)))
        .addMethod(
          getRevokeApiKeyMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Auth.RevokeApiKeyRequest,
              talon.v1.Auth.RevokeApiKeyResponse>(
                service, METHODID_REVOKE_API_KEY)))
        .addMethod(
          getExchangeApiKeyMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Auth.ExchangeApiKeyRequest,
              talon.v1.Auth.ExchangeApiKeyResponse>(
                service, METHODID_EXCHANGE_API_KEY)))
        .build();
  }

  private static abstract class AuthServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    AuthServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return talon.v1.Auth.getDescriptor();
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
              .addMethod(getMintAccessTokenMethod())
              .addMethod(getCreateApiKeyMethod())
              .addMethod(getListApiKeysMethod())
              .addMethod(getRevokeApiKeyMethod())
              .addMethod(getExchangeApiKeyMethod())
              .build();
        }
      }
    }
    return result;
  }
}
