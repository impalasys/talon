package talon.v1;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class ResourceServiceGrpc {

  private ResourceServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.v1.ResourceService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<talon.v1.Api.CreateResourceRequest,
      talon.v1.Api.ResourceResponse> getCreateMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Create",
      requestType = talon.v1.Api.CreateResourceRequest.class,
      responseType = talon.v1.Api.ResourceResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Api.CreateResourceRequest,
      talon.v1.Api.ResourceResponse> getCreateMethod() {
    io.grpc.MethodDescriptor<talon.v1.Api.CreateResourceRequest, talon.v1.Api.ResourceResponse> getCreateMethod;
    if ((getCreateMethod = ResourceServiceGrpc.getCreateMethod) == null) {
      synchronized (ResourceServiceGrpc.class) {
        if ((getCreateMethod = ResourceServiceGrpc.getCreateMethod) == null) {
          ResourceServiceGrpc.getCreateMethod = getCreateMethod =
              io.grpc.MethodDescriptor.<talon.v1.Api.CreateResourceRequest, talon.v1.Api.ResourceResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Create"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.CreateResourceRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.ResourceResponse.getDefaultInstance()))
              .setSchemaDescriptor(new ResourceServiceMethodDescriptorSupplier("Create"))
              .build();
        }
      }
    }
    return getCreateMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Api.GetResourceRequest,
      talon.v1.Api.ResourceResponse> getGetMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Get",
      requestType = talon.v1.Api.GetResourceRequest.class,
      responseType = talon.v1.Api.ResourceResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Api.GetResourceRequest,
      talon.v1.Api.ResourceResponse> getGetMethod() {
    io.grpc.MethodDescriptor<talon.v1.Api.GetResourceRequest, talon.v1.Api.ResourceResponse> getGetMethod;
    if ((getGetMethod = ResourceServiceGrpc.getGetMethod) == null) {
      synchronized (ResourceServiceGrpc.class) {
        if ((getGetMethod = ResourceServiceGrpc.getGetMethod) == null) {
          ResourceServiceGrpc.getGetMethod = getGetMethod =
              io.grpc.MethodDescriptor.<talon.v1.Api.GetResourceRequest, talon.v1.Api.ResourceResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Get"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.GetResourceRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.ResourceResponse.getDefaultInstance()))
              .setSchemaDescriptor(new ResourceServiceMethodDescriptorSupplier("Get"))
              .build();
        }
      }
    }
    return getGetMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Api.ListResourcesRequest,
      talon.v1.Api.ListResourcesResponse> getListMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "List",
      requestType = talon.v1.Api.ListResourcesRequest.class,
      responseType = talon.v1.Api.ListResourcesResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Api.ListResourcesRequest,
      talon.v1.Api.ListResourcesResponse> getListMethod() {
    io.grpc.MethodDescriptor<talon.v1.Api.ListResourcesRequest, talon.v1.Api.ListResourcesResponse> getListMethod;
    if ((getListMethod = ResourceServiceGrpc.getListMethod) == null) {
      synchronized (ResourceServiceGrpc.class) {
        if ((getListMethod = ResourceServiceGrpc.getListMethod) == null) {
          ResourceServiceGrpc.getListMethod = getListMethod =
              io.grpc.MethodDescriptor.<talon.v1.Api.ListResourcesRequest, talon.v1.Api.ListResourcesResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "List"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.ListResourcesRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.ListResourcesResponse.getDefaultInstance()))
              .setSchemaDescriptor(new ResourceServiceMethodDescriptorSupplier("List"))
              .build();
        }
      }
    }
    return getListMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Api.DeleteResourceRequest,
      talon.v1.Api.DeleteResourceResponse> getDeleteMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "Delete",
      requestType = talon.v1.Api.DeleteResourceRequest.class,
      responseType = talon.v1.Api.DeleteResourceResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Api.DeleteResourceRequest,
      talon.v1.Api.DeleteResourceResponse> getDeleteMethod() {
    io.grpc.MethodDescriptor<talon.v1.Api.DeleteResourceRequest, talon.v1.Api.DeleteResourceResponse> getDeleteMethod;
    if ((getDeleteMethod = ResourceServiceGrpc.getDeleteMethod) == null) {
      synchronized (ResourceServiceGrpc.class) {
        if ((getDeleteMethod = ResourceServiceGrpc.getDeleteMethod) == null) {
          ResourceServiceGrpc.getDeleteMethod = getDeleteMethod =
              io.grpc.MethodDescriptor.<talon.v1.Api.DeleteResourceRequest, talon.v1.Api.DeleteResourceResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "Delete"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.DeleteResourceRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Api.DeleteResourceResponse.getDefaultInstance()))
              .setSchemaDescriptor(new ResourceServiceMethodDescriptorSupplier("Delete"))
              .build();
        }
      }
    }
    return getDeleteMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static ResourceServiceStub newStub(io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ResourceServiceStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ResourceServiceStub>() {
        @java.lang.Override
        public ResourceServiceStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ResourceServiceStub(channel, callOptions);
        }
      };
    return ResourceServiceStub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports all types of calls on the service
   */
  public static ResourceServiceBlockingV2Stub newBlockingV2Stub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ResourceServiceBlockingV2Stub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ResourceServiceBlockingV2Stub>() {
        @java.lang.Override
        public ResourceServiceBlockingV2Stub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ResourceServiceBlockingV2Stub(channel, callOptions);
        }
      };
    return ResourceServiceBlockingV2Stub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static ResourceServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ResourceServiceBlockingStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ResourceServiceBlockingStub>() {
        @java.lang.Override
        public ResourceServiceBlockingStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ResourceServiceBlockingStub(channel, callOptions);
        }
      };
    return ResourceServiceBlockingStub.newStub(factory, channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static ResourceServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<ResourceServiceFutureStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<ResourceServiceFutureStub>() {
        @java.lang.Override
        public ResourceServiceFutureStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new ResourceServiceFutureStub(channel, callOptions);
        }
      };
    return ResourceServiceFutureStub.newStub(factory, channel);
  }

  /**
   */
  public interface AsyncService {

    /**
     */
    default void create(talon.v1.Api.CreateResourceRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.ResourceResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateMethod(), responseObserver);
    }

    /**
     */
    default void get(talon.v1.Api.GetResourceRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.ResourceResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetMethod(), responseObserver);
    }

    /**
     */
    default void list(talon.v1.Api.ListResourcesRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.ListResourcesResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListMethod(), responseObserver);
    }

    /**
     */
    default void delete(talon.v1.Api.DeleteResourceRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.DeleteResourceResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteMethod(), responseObserver);
    }
  }

  /**
   * Base class for the server implementation of the service ResourceService.
   */
  public static abstract class ResourceServiceImplBase
      implements io.grpc.BindableService, AsyncService {

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return ResourceServiceGrpc.bindService(this);
    }
  }

  /**
   * A stub to allow clients to do asynchronous rpc calls to service ResourceService.
   */
  public static final class ResourceServiceStub
      extends io.grpc.stub.AbstractAsyncStub<ResourceServiceStub> {
    private ResourceServiceStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ResourceServiceStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ResourceServiceStub(channel, callOptions);
    }

    /**
     */
    public void create(talon.v1.Api.CreateResourceRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.ResourceResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void get(talon.v1.Api.GetResourceRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.ResourceResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void list(talon.v1.Api.ListResourcesRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.ListResourcesResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void delete(talon.v1.Api.DeleteResourceRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Api.DeleteResourceResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   * A stub to allow clients to do synchronous rpc calls to service ResourceService.
   */
  public static final class ResourceServiceBlockingV2Stub
      extends io.grpc.stub.AbstractBlockingStub<ResourceServiceBlockingV2Stub> {
    private ResourceServiceBlockingV2Stub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ResourceServiceBlockingV2Stub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ResourceServiceBlockingV2Stub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Api.ResourceResponse create(talon.v1.Api.CreateResourceRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Api.ResourceResponse get(talon.v1.Api.GetResourceRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Api.ListResourcesResponse list(talon.v1.Api.ListResourcesRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Api.DeleteResourceResponse delete(talon.v1.Api.DeleteResourceRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do limited synchronous rpc calls to service ResourceService.
   */
  public static final class ResourceServiceBlockingStub
      extends io.grpc.stub.AbstractBlockingStub<ResourceServiceBlockingStub> {
    private ResourceServiceBlockingStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ResourceServiceBlockingStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ResourceServiceBlockingStub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Api.ResourceResponse create(talon.v1.Api.CreateResourceRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Api.ResourceResponse get(talon.v1.Api.GetResourceRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Api.ListResourcesResponse list(talon.v1.Api.ListResourcesRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Api.DeleteResourceResponse delete(talon.v1.Api.DeleteResourceRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do ListenableFuture-style rpc calls to service ResourceService.
   */
  public static final class ResourceServiceFutureStub
      extends io.grpc.stub.AbstractFutureStub<ResourceServiceFutureStub> {
    private ResourceServiceFutureStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected ResourceServiceFutureStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new ResourceServiceFutureStub(channel, callOptions);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Api.ResourceResponse> create(
        talon.v1.Api.CreateResourceRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Api.ResourceResponse> get(
        talon.v1.Api.GetResourceRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Api.ListResourcesResponse> list(
        talon.v1.Api.ListResourcesRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Api.DeleteResourceResponse> delete(
        talon.v1.Api.DeleteResourceRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_CREATE = 0;
  private static final int METHODID_GET = 1;
  private static final int METHODID_LIST = 2;
  private static final int METHODID_DELETE = 3;

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
          serviceImpl.create((talon.v1.Api.CreateResourceRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Api.ResourceResponse>) responseObserver);
          break;
        case METHODID_GET:
          serviceImpl.get((talon.v1.Api.GetResourceRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Api.ResourceResponse>) responseObserver);
          break;
        case METHODID_LIST:
          serviceImpl.list((talon.v1.Api.ListResourcesRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Api.ListResourcesResponse>) responseObserver);
          break;
        case METHODID_DELETE:
          serviceImpl.delete((talon.v1.Api.DeleteResourceRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Api.DeleteResourceResponse>) responseObserver);
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
              talon.v1.Api.CreateResourceRequest,
              talon.v1.Api.ResourceResponse>(
                service, METHODID_CREATE)))
        .addMethod(
          getGetMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Api.GetResourceRequest,
              talon.v1.Api.ResourceResponse>(
                service, METHODID_GET)))
        .addMethod(
          getListMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Api.ListResourcesRequest,
              talon.v1.Api.ListResourcesResponse>(
                service, METHODID_LIST)))
        .addMethod(
          getDeleteMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Api.DeleteResourceRequest,
              talon.v1.Api.DeleteResourceResponse>(
                service, METHODID_DELETE)))
        .build();
  }

  private static abstract class ResourceServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    ResourceServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return talon.v1.Api.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("ResourceService");
    }
  }

  private static final class ResourceServiceFileDescriptorSupplier
      extends ResourceServiceBaseDescriptorSupplier {
    ResourceServiceFileDescriptorSupplier() {}
  }

  private static final class ResourceServiceMethodDescriptorSupplier
      extends ResourceServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final java.lang.String methodName;

    ResourceServiceMethodDescriptorSupplier(java.lang.String methodName) {
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
      synchronized (ResourceServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new ResourceServiceFileDescriptorSupplier())
              .addMethod(getCreateMethod())
              .addMethod(getGetMethod())
              .addMethod(getListMethod())
              .addMethod(getDeleteMethod())
              .build();
        }
      }
    }
    return result;
  }
}
