package talon.v1;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class NamespaceServiceGrpc {

  private NamespaceServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.v1.NamespaceService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<talon.v1.Namespaces.CreateNamespaceRequest,
      talon.v1.Namespaces.NamespaceResponse> getCreateMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Create",
      requestType = talon.v1.Namespaces.CreateNamespaceRequest.class,
      responseType = talon.v1.Namespaces.NamespaceResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Namespaces.CreateNamespaceRequest,
      talon.v1.Namespaces.NamespaceResponse> getCreateMethod() {
    io.grpc.MethodDescriptor<talon.v1.Namespaces.CreateNamespaceRequest, talon.v1.Namespaces.NamespaceResponse> getCreateMethod;
    if ((getCreateMethod = NamespaceServiceGrpc.getCreateMethod) == null) {
      synchronized (NamespaceServiceGrpc.class) {
        if ((getCreateMethod = NamespaceServiceGrpc.getCreateMethod) == null) {
          NamespaceServiceGrpc.getCreateMethod = getCreateMethod =
              io.grpc.MethodDescriptor.<talon.v1.Namespaces.CreateNamespaceRequest, talon.v1.Namespaces.NamespaceResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Create"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Namespaces.CreateNamespaceRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Namespaces.NamespaceResponse.getDefaultInstance()))
              .setSchemaDescriptor(new NamespaceServiceMethodDescriptorSupplier("Create"))
              .build();
        }
      }
    }
    return getCreateMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Namespaces.GetNamespaceRequest,
      talon.v1.Namespaces.NamespaceResponse> getGetMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Get",
      requestType = talon.v1.Namespaces.GetNamespaceRequest.class,
      responseType = talon.v1.Namespaces.NamespaceResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Namespaces.GetNamespaceRequest,
      talon.v1.Namespaces.NamespaceResponse> getGetMethod() {
    io.grpc.MethodDescriptor<talon.v1.Namespaces.GetNamespaceRequest, talon.v1.Namespaces.NamespaceResponse> getGetMethod;
    if ((getGetMethod = NamespaceServiceGrpc.getGetMethod) == null) {
      synchronized (NamespaceServiceGrpc.class) {
        if ((getGetMethod = NamespaceServiceGrpc.getGetMethod) == null) {
          NamespaceServiceGrpc.getGetMethod = getGetMethod =
              io.grpc.MethodDescriptor.<talon.v1.Namespaces.GetNamespaceRequest, talon.v1.Namespaces.NamespaceResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Get"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Namespaces.GetNamespaceRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Namespaces.NamespaceResponse.getDefaultInstance()))
              .setSchemaDescriptor(new NamespaceServiceMethodDescriptorSupplier("Get"))
              .build();
        }
      }
    }
    return getGetMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Namespaces.DeleteNamespaceRequest,
      talon.v1.Namespaces.NamespaceResponse> getDeleteMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Delete",
      requestType = talon.v1.Namespaces.DeleteNamespaceRequest.class,
      responseType = talon.v1.Namespaces.NamespaceResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Namespaces.DeleteNamespaceRequest,
      talon.v1.Namespaces.NamespaceResponse> getDeleteMethod() {
    io.grpc.MethodDescriptor<talon.v1.Namespaces.DeleteNamespaceRequest, talon.v1.Namespaces.NamespaceResponse> getDeleteMethod;
    if ((getDeleteMethod = NamespaceServiceGrpc.getDeleteMethod) == null) {
      synchronized (NamespaceServiceGrpc.class) {
        if ((getDeleteMethod = NamespaceServiceGrpc.getDeleteMethod) == null) {
          NamespaceServiceGrpc.getDeleteMethod = getDeleteMethod =
              io.grpc.MethodDescriptor.<talon.v1.Namespaces.DeleteNamespaceRequest, talon.v1.Namespaces.NamespaceResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Delete"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Namespaces.DeleteNamespaceRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Namespaces.NamespaceResponse.getDefaultInstance()))
              .setSchemaDescriptor(new NamespaceServiceMethodDescriptorSupplier("Delete"))
              .build();
        }
      }
    }
    return getDeleteMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Namespaces.ListNamespacesRequest,
      talon.v1.Namespaces.ListNamespacesResponse> getListMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "List",
      requestType = talon.v1.Namespaces.ListNamespacesRequest.class,
      responseType = talon.v1.Namespaces.ListNamespacesResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Namespaces.ListNamespacesRequest,
      talon.v1.Namespaces.ListNamespacesResponse> getListMethod() {
    io.grpc.MethodDescriptor<talon.v1.Namespaces.ListNamespacesRequest, talon.v1.Namespaces.ListNamespacesResponse> getListMethod;
    if ((getListMethod = NamespaceServiceGrpc.getListMethod) == null) {
      synchronized (NamespaceServiceGrpc.class) {
        if ((getListMethod = NamespaceServiceGrpc.getListMethod) == null) {
          NamespaceServiceGrpc.getListMethod = getListMethod =
              io.grpc.MethodDescriptor.<talon.v1.Namespaces.ListNamespacesRequest, talon.v1.Namespaces.ListNamespacesResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "List"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Namespaces.ListNamespacesRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Namespaces.ListNamespacesResponse.getDefaultInstance()))
              .setSchemaDescriptor(new NamespaceServiceMethodDescriptorSupplier("List"))
              .build();
        }
      }
    }
    return getListMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static NamespaceServiceStub newStub(io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<NamespaceServiceStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<NamespaceServiceStub>() {
        @java.lang.Override
        public NamespaceServiceStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new NamespaceServiceStub(channel, callOptions);
        }
      };
    return NamespaceServiceStub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports all types of calls on the service
   */
  public static NamespaceServiceBlockingV2Stub newBlockingV2Stub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<NamespaceServiceBlockingV2Stub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<NamespaceServiceBlockingV2Stub>() {
        @java.lang.Override
        public NamespaceServiceBlockingV2Stub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new NamespaceServiceBlockingV2Stub(channel, callOptions);
        }
      };
    return NamespaceServiceBlockingV2Stub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static NamespaceServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<NamespaceServiceBlockingStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<NamespaceServiceBlockingStub>() {
        @java.lang.Override
        public NamespaceServiceBlockingStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new NamespaceServiceBlockingStub(channel, callOptions);
        }
      };
    return NamespaceServiceBlockingStub.newStub(factory, channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static NamespaceServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<NamespaceServiceFutureStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<NamespaceServiceFutureStub>() {
        @java.lang.Override
        public NamespaceServiceFutureStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new NamespaceServiceFutureStub(channel, callOptions);
        }
      };
    return NamespaceServiceFutureStub.newStub(factory, channel);
  }

  /**
   */
  public interface AsyncService {

    /**
     */
    default void create(talon.v1.Namespaces.CreateNamespaceRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Namespaces.NamespaceResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateMethod(), responseObserver);
    }

    /**
     */
    default void get(talon.v1.Namespaces.GetNamespaceRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Namespaces.NamespaceResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetMethod(), responseObserver);
    }

    /**
     */
    default void delete(talon.v1.Namespaces.DeleteNamespaceRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Namespaces.NamespaceResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteMethod(), responseObserver);
    }

    /**
     */
    default void list(talon.v1.Namespaces.ListNamespacesRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Namespaces.ListNamespacesResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListMethod(), responseObserver);
    }
  }

  /**
   * Base class for the server implementation of the service NamespaceService.
   */
  public static abstract class NamespaceServiceImplBase
      implements io.grpc.BindableService, AsyncService {

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return NamespaceServiceGrpc.bindService(this);
    }
  }

  /**
   * A stub to allow clients to do asynchronous rpc calls to service NamespaceService.
   */
  public static final class NamespaceServiceStub
      extends io.grpc.stub.AbstractAsyncStub<NamespaceServiceStub> {
    private NamespaceServiceStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected NamespaceServiceStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new NamespaceServiceStub(channel, callOptions);
    }

    /**
     */
    public void create(talon.v1.Namespaces.CreateNamespaceRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Namespaces.NamespaceResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void get(talon.v1.Namespaces.GetNamespaceRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Namespaces.NamespaceResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void delete(talon.v1.Namespaces.DeleteNamespaceRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Namespaces.NamespaceResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void list(talon.v1.Namespaces.ListNamespacesRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Namespaces.ListNamespacesResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   * A stub to allow clients to do synchronous rpc calls to service NamespaceService.
   */
  public static final class NamespaceServiceBlockingV2Stub
      extends io.grpc.stub.AbstractBlockingStub<NamespaceServiceBlockingV2Stub> {
    private NamespaceServiceBlockingV2Stub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected NamespaceServiceBlockingV2Stub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new NamespaceServiceBlockingV2Stub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Namespaces.NamespaceResponse create(talon.v1.Namespaces.CreateNamespaceRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Namespaces.NamespaceResponse get(talon.v1.Namespaces.GetNamespaceRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Namespaces.NamespaceResponse delete(talon.v1.Namespaces.DeleteNamespaceRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Namespaces.ListNamespacesResponse list(talon.v1.Namespaces.ListNamespacesRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do limited synchronous rpc calls to service NamespaceService.
   */
  public static final class NamespaceServiceBlockingStub
      extends io.grpc.stub.AbstractBlockingStub<NamespaceServiceBlockingStub> {
    private NamespaceServiceBlockingStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected NamespaceServiceBlockingStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new NamespaceServiceBlockingStub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Namespaces.NamespaceResponse create(talon.v1.Namespaces.CreateNamespaceRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Namespaces.NamespaceResponse get(talon.v1.Namespaces.GetNamespaceRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Namespaces.NamespaceResponse delete(talon.v1.Namespaces.DeleteNamespaceRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Namespaces.ListNamespacesResponse list(talon.v1.Namespaces.ListNamespacesRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do ListenableFuture-style rpc calls to service NamespaceService.
   */
  public static final class NamespaceServiceFutureStub
      extends io.grpc.stub.AbstractFutureStub<NamespaceServiceFutureStub> {
    private NamespaceServiceFutureStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected NamespaceServiceFutureStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new NamespaceServiceFutureStub(channel, callOptions);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Namespaces.NamespaceResponse> create(
        talon.v1.Namespaces.CreateNamespaceRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Namespaces.NamespaceResponse> get(
        talon.v1.Namespaces.GetNamespaceRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Namespaces.NamespaceResponse> delete(
        talon.v1.Namespaces.DeleteNamespaceRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Namespaces.ListNamespacesResponse> list(
        talon.v1.Namespaces.ListNamespacesRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_CREATE = 0;
  private static final int METHODID_GET = 1;
  private static final int METHODID_DELETE = 2;
  private static final int METHODID_LIST = 3;

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
        case METHODID_CREATE:
          serviceImpl.create((talon.v1.Namespaces.CreateNamespaceRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Namespaces.NamespaceResponse>) responseObserver);
          break;
        case METHODID_GET:
          serviceImpl.get((talon.v1.Namespaces.GetNamespaceRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Namespaces.NamespaceResponse>) responseObserver);
          break;
        case METHODID_DELETE:
          serviceImpl.delete((talon.v1.Namespaces.DeleteNamespaceRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Namespaces.NamespaceResponse>) responseObserver);
          break;
        case METHODID_LIST:
          serviceImpl.list((talon.v1.Namespaces.ListNamespacesRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Namespaces.ListNamespacesResponse>) responseObserver);
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
          getCreateMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Namespaces.CreateNamespaceRequest,
              talon.v1.Namespaces.NamespaceResponse>(
                service, METHODID_CREATE)))
        .addMethod(
          getGetMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Namespaces.GetNamespaceRequest,
              talon.v1.Namespaces.NamespaceResponse>(
                service, METHODID_GET)))
        .addMethod(
          getDeleteMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Namespaces.DeleteNamespaceRequest,
              talon.v1.Namespaces.NamespaceResponse>(
                service, METHODID_DELETE)))
        .addMethod(
          getListMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Namespaces.ListNamespacesRequest,
              talon.v1.Namespaces.ListNamespacesResponse>(
                service, METHODID_LIST)))
        .build();
  }

  private static abstract class NamespaceServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    NamespaceServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return talon.v1.Namespaces.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("NamespaceService");
    }
  }

  private static final class NamespaceServiceFileDescriptorSupplier
      extends NamespaceServiceBaseDescriptorSupplier {
    NamespaceServiceFileDescriptorSupplier() {}
  }

  private static final class NamespaceServiceMethodDescriptorSupplier
      extends NamespaceServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final java.lang.String methodName;

    NamespaceServiceMethodDescriptorSupplier(java.lang.String methodName) {
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
      synchronized (NamespaceServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new NamespaceServiceFileDescriptorSupplier())
              .addMethod(getCreateMethod())
              .addMethod(getGetMethod())
              .addMethod(getDeleteMethod())
              .addMethod(getListMethod())
              .build();
        }
      }
    }
    return result;
  }
}
