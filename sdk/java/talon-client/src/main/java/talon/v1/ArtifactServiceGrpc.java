package talon.v1;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class ArtifactServiceGrpc {

  private ArtifactServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.v1.ArtifactService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<talon.v1.Files.ReadArtifactRequest,
      talon.v1.Files.ReadArtifactResponse> getReadArtifactMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ReadArtifact",
      requestType = talon.v1.Files.ReadArtifactRequest.class,
      responseType = talon.v1.Files.ReadArtifactResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Files.ReadArtifactRequest,
      talon.v1.Files.ReadArtifactResponse> getReadArtifactMethod() {
    io.grpc.MethodDescriptor<talon.v1.Files.ReadArtifactRequest, talon.v1.Files.ReadArtifactResponse> getReadArtifactMethod;
    if ((getReadArtifactMethod = ArtifactServiceGrpc.getReadArtifactMethod) == null) {
      synchronized (ArtifactServiceGrpc.class) {
        if ((getReadArtifactMethod = ArtifactServiceGrpc.getReadArtifactMethod) == null) {
          ArtifactServiceGrpc.getReadArtifactMethod = getReadArtifactMethod =
              io.grpc.MethodDescriptor.<talon.v1.Files.ReadArtifactRequest, talon.v1.Files.ReadArtifactResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ReadArtifact"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.ReadArtifactRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.ReadArtifactResponse.getDefaultInstance()))
              .setSchemaDescriptor(new ArtifactServiceMethodDescriptorSupplier("ReadArtifact"))
              .build();
        }
      }
    }
    return getReadArtifactMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Files.GetArtifactMetadataRequest,
      talon.v1.Files.ArtifactResponse> getGetArtifactMetadataMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetArtifactMetadata",
      requestType = talon.v1.Files.GetArtifactMetadataRequest.class,
      responseType = talon.v1.Files.ArtifactResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Files.GetArtifactMetadataRequest,
      talon.v1.Files.ArtifactResponse> getGetArtifactMetadataMethod() {
    io.grpc.MethodDescriptor<talon.v1.Files.GetArtifactMetadataRequest, talon.v1.Files.ArtifactResponse> getGetArtifactMetadataMethod;
    if ((getGetArtifactMetadataMethod = ArtifactServiceGrpc.getGetArtifactMetadataMethod) == null) {
      synchronized (ArtifactServiceGrpc.class) {
        if ((getGetArtifactMetadataMethod = ArtifactServiceGrpc.getGetArtifactMetadataMethod) == null) {
          ArtifactServiceGrpc.getGetArtifactMetadataMethod = getGetArtifactMetadataMethod =
              io.grpc.MethodDescriptor.<talon.v1.Files.GetArtifactMetadataRequest, talon.v1.Files.ArtifactResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetArtifactMetadata"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.GetArtifactMetadataRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.ArtifactResponse.getDefaultInstance()))
              .setSchemaDescriptor(new ArtifactServiceMethodDescriptorSupplier("GetArtifactMetadata"))
              .build();
        }
      }
    }
    return getGetArtifactMetadataMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Files.ListArtifactsRequest,
      talon.v1.Files.ListArtifactsResponse> getListArtifactsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListArtifacts",
      requestType = talon.v1.Files.ListArtifactsRequest.class,
      responseType = talon.v1.Files.ListArtifactsResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Files.ListArtifactsRequest,
      talon.v1.Files.ListArtifactsResponse> getListArtifactsMethod() {
    io.grpc.MethodDescriptor<talon.v1.Files.ListArtifactsRequest, talon.v1.Files.ListArtifactsResponse> getListArtifactsMethod;
    if ((getListArtifactsMethod = ArtifactServiceGrpc.getListArtifactsMethod) == null) {
      synchronized (ArtifactServiceGrpc.class) {
        if ((getListArtifactsMethod = ArtifactServiceGrpc.getListArtifactsMethod) == null) {
          ArtifactServiceGrpc.getListArtifactsMethod = getListArtifactsMethod =
              io.grpc.MethodDescriptor.<talon.v1.Files.ListArtifactsRequest, talon.v1.Files.ListArtifactsResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListArtifacts"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.ListArtifactsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.ListArtifactsResponse.getDefaultInstance()))
              .setSchemaDescriptor(new ArtifactServiceMethodDescriptorSupplier("ListArtifacts"))
              .build();
        }
      }
    }
    return getListArtifactsMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Files.GrantArtifactRequest,
      talon.v1.Files.ArtifactUriResponse> getGrantArtifactMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GrantArtifact",
      requestType = talon.v1.Files.GrantArtifactRequest.class,
      responseType = talon.v1.Files.ArtifactUriResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Files.GrantArtifactRequest,
      talon.v1.Files.ArtifactUriResponse> getGrantArtifactMethod() {
    io.grpc.MethodDescriptor<talon.v1.Files.GrantArtifactRequest, talon.v1.Files.ArtifactUriResponse> getGrantArtifactMethod;
    if ((getGrantArtifactMethod = ArtifactServiceGrpc.getGrantArtifactMethod) == null) {
      synchronized (ArtifactServiceGrpc.class) {
        if ((getGrantArtifactMethod = ArtifactServiceGrpc.getGrantArtifactMethod) == null) {
          ArtifactServiceGrpc.getGrantArtifactMethod = getGrantArtifactMethod =
              io.grpc.MethodDescriptor.<talon.v1.Files.GrantArtifactRequest, talon.v1.Files.ArtifactUriResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GrantArtifact"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.GrantArtifactRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.ArtifactUriResponse.getDefaultInstance()))
              .setSchemaDescriptor(new ArtifactServiceMethodDescriptorSupplier("GrantArtifact"))
              .build();
        }
      }
    }
    return getGrantArtifactMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static ArtifactServiceStub newStub(io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ArtifactServiceStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ArtifactServiceStub>() {
        @java.lang.Override
        public ArtifactServiceStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ArtifactServiceStub(channel, callOptions);
        }
      };
    return ArtifactServiceStub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports all types of calls on the service
   */
  public static ArtifactServiceBlockingV2Stub newBlockingV2Stub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ArtifactServiceBlockingV2Stub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ArtifactServiceBlockingV2Stub>() {
        @java.lang.Override
        public ArtifactServiceBlockingV2Stub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ArtifactServiceBlockingV2Stub(channel, callOptions);
        }
      };
    return ArtifactServiceBlockingV2Stub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static ArtifactServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ArtifactServiceBlockingStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ArtifactServiceBlockingStub>() {
        @java.lang.Override
        public ArtifactServiceBlockingStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ArtifactServiceBlockingStub(channel, callOptions);
        }
      };
    return ArtifactServiceBlockingStub.newStub(factory, channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static ArtifactServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ArtifactServiceFutureStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ArtifactServiceFutureStub>() {
        @java.lang.Override
        public ArtifactServiceFutureStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ArtifactServiceFutureStub(channel, callOptions);
        }
      };
    return ArtifactServiceFutureStub.newStub(factory, channel);
  }

  /**
   */
  public interface AsyncService {

    /**
     * <pre>
     * Artifact creation is handled by the runtime/tooling for an active session.
     * This service only exposes URI-based artifact reads, metadata, listing,
     * and access grant operations.
     * </pre>
     */
    default void readArtifact(talon.v1.Files.ReadArtifactRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.ReadArtifactResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getReadArtifactMethod(), responseObserver);
    }

    /**
     */
    default void getArtifactMetadata(talon.v1.Files.GetArtifactMetadataRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.ArtifactResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetArtifactMetadataMethod(), responseObserver);
    }

    /**
     */
    default void listArtifacts(talon.v1.Files.ListArtifactsRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.ListArtifactsResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListArtifactsMethod(), responseObserver);
    }

    /**
     */
    default void grantArtifact(talon.v1.Files.GrantArtifactRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.ArtifactUriResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGrantArtifactMethod(), responseObserver);
    }
  }

  /**
   * Base class for the server implementation of the service ArtifactService.
   */
  public static abstract class ArtifactServiceImplBase
      implements io.grpc.BindableService, AsyncService {

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return ArtifactServiceGrpc.bindService(this);
    }
  }

  /**
   * A stub to allow clients to do asynchronous rpc calls to service ArtifactService.
   */
  public static final class ArtifactServiceStub
      extends io.grpc.stub.AbstractAsyncStub<ArtifactServiceStub> {
    private ArtifactServiceStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ArtifactServiceStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ArtifactServiceStub(channel, callOptions);
    }

    /**
     * <pre>
     * Artifact creation is handled by the runtime/tooling for an active session.
     * This service only exposes URI-based artifact reads, metadata, listing,
     * and access grant operations.
     * </pre>
     */
    public void readArtifact(talon.v1.Files.ReadArtifactRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.ReadArtifactResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getReadArtifactMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getArtifactMetadata(talon.v1.Files.GetArtifactMetadataRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.ArtifactResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetArtifactMetadataMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listArtifacts(talon.v1.Files.ListArtifactsRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.ListArtifactsResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListArtifactsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void grantArtifact(talon.v1.Files.GrantArtifactRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.ArtifactUriResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGrantArtifactMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   * A stub to allow clients to do synchronous rpc calls to service ArtifactService.
   */
  public static final class ArtifactServiceBlockingV2Stub
      extends io.grpc.stub.AbstractBlockingStub<ArtifactServiceBlockingV2Stub> {
    private ArtifactServiceBlockingV2Stub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ArtifactServiceBlockingV2Stub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ArtifactServiceBlockingV2Stub(channel, callOptions);
    }

    /**
     * <pre>
     * Artifact creation is handled by the runtime/tooling for an active session.
     * This service only exposes URI-based artifact reads, metadata, listing,
     * and access grant operations.
     * </pre>
     */
    public talon.v1.Files.ReadArtifactResponse readArtifact(talon.v1.Files.ReadArtifactRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getReadArtifactMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.ArtifactResponse getArtifactMetadata(talon.v1.Files.GetArtifactMetadataRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetArtifactMetadataMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.ListArtifactsResponse listArtifacts(talon.v1.Files.ListArtifactsRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListArtifactsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.ArtifactUriResponse grantArtifact(talon.v1.Files.GrantArtifactRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGrantArtifactMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do limited synchronous rpc calls to service ArtifactService.
   */
  public static final class ArtifactServiceBlockingStub
      extends io.grpc.stub.AbstractBlockingStub<ArtifactServiceBlockingStub> {
    private ArtifactServiceBlockingStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ArtifactServiceBlockingStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ArtifactServiceBlockingStub(channel, callOptions);
    }

    /**
     * <pre>
     * Artifact creation is handled by the runtime/tooling for an active session.
     * This service only exposes URI-based artifact reads, metadata, listing,
     * and access grant operations.
     * </pre>
     */
    public talon.v1.Files.ReadArtifactResponse readArtifact(talon.v1.Files.ReadArtifactRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getReadArtifactMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.ArtifactResponse getArtifactMetadata(talon.v1.Files.GetArtifactMetadataRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetArtifactMetadataMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.ListArtifactsResponse listArtifacts(talon.v1.Files.ListArtifactsRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListArtifactsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.ArtifactUriResponse grantArtifact(talon.v1.Files.GrantArtifactRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGrantArtifactMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do ListenableFuture-style rpc calls to service ArtifactService.
   */
  public static final class ArtifactServiceFutureStub
      extends io.grpc.stub.AbstractFutureStub<ArtifactServiceFutureStub> {
    private ArtifactServiceFutureStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ArtifactServiceFutureStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ArtifactServiceFutureStub(channel, callOptions);
    }

    /**
     * <pre>
     * Artifact creation is handled by the runtime/tooling for an active session.
     * This service only exposes URI-based artifact reads, metadata, listing,
     * and access grant operations.
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Files.ReadArtifactResponse> readArtifact(
        talon.v1.Files.ReadArtifactRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getReadArtifactMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Files.ArtifactResponse> getArtifactMetadata(
        talon.v1.Files.GetArtifactMetadataRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetArtifactMetadataMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Files.ListArtifactsResponse> listArtifacts(
        talon.v1.Files.ListArtifactsRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListArtifactsMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Files.ArtifactUriResponse> grantArtifact(
        talon.v1.Files.GrantArtifactRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGrantArtifactMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_READ_ARTIFACT = 0;
  private static final int METHODID_GET_ARTIFACT_METADATA = 1;
  private static final int METHODID_LIST_ARTIFACTS = 2;
  private static final int METHODID_GRANT_ARTIFACT = 3;

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
        case METHODID_READ_ARTIFACT:
          serviceImpl.readArtifact((talon.v1.Files.ReadArtifactRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Files.ReadArtifactResponse>) responseObserver);
          break;
        case METHODID_GET_ARTIFACT_METADATA:
          serviceImpl.getArtifactMetadata((talon.v1.Files.GetArtifactMetadataRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Files.ArtifactResponse>) responseObserver);
          break;
        case METHODID_LIST_ARTIFACTS:
          serviceImpl.listArtifacts((talon.v1.Files.ListArtifactsRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Files.ListArtifactsResponse>) responseObserver);
          break;
        case METHODID_GRANT_ARTIFACT:
          serviceImpl.grantArtifact((talon.v1.Files.GrantArtifactRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Files.ArtifactUriResponse>) responseObserver);
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
          getReadArtifactMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Files.ReadArtifactRequest,
              talon.v1.Files.ReadArtifactResponse>(
                service, METHODID_READ_ARTIFACT)))
        .addMethod(
          getGetArtifactMetadataMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Files.GetArtifactMetadataRequest,
              talon.v1.Files.ArtifactResponse>(
                service, METHODID_GET_ARTIFACT_METADATA)))
        .addMethod(
          getListArtifactsMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Files.ListArtifactsRequest,
              talon.v1.Files.ListArtifactsResponse>(
                service, METHODID_LIST_ARTIFACTS)))
        .addMethod(
          getGrantArtifactMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Files.GrantArtifactRequest,
              talon.v1.Files.ArtifactUriResponse>(
                service, METHODID_GRANT_ARTIFACT)))
        .build();
  }

  private static abstract class ArtifactServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    ArtifactServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return talon.v1.Files.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("ArtifactService");
    }
  }

  private static final class ArtifactServiceFileDescriptorSupplier
      extends ArtifactServiceBaseDescriptorSupplier {
    ArtifactServiceFileDescriptorSupplier() {}
  }

  private static final class ArtifactServiceMethodDescriptorSupplier
      extends ArtifactServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final java.lang.String methodName;

    ArtifactServiceMethodDescriptorSupplier(java.lang.String methodName) {
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
      synchronized (ArtifactServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new ArtifactServiceFileDescriptorSupplier())
              .addMethod(getReadArtifactMethod())
              .addMethod(getGetArtifactMetadataMethod())
              .addMethod(getListArtifactsMethod())
              .addMethod(getGrantArtifactMethod())
              .build();
        }
      }
    }
    return result;
  }
}
