package talon.v1;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class WorkflowServiceGrpc {

  private WorkflowServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.v1.WorkflowService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<talon.v1.Workflows.CreateWorkflowRunRequest,
      talon.v1.Workflows.WorkflowRunResponse> getCreateRunMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateRun",
      requestType = talon.v1.Workflows.CreateWorkflowRunRequest.class,
      responseType = talon.v1.Workflows.WorkflowRunResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Workflows.CreateWorkflowRunRequest,
      talon.v1.Workflows.WorkflowRunResponse> getCreateRunMethod() {
    io.grpc.MethodDescriptor<talon.v1.Workflows.CreateWorkflowRunRequest, talon.v1.Workflows.WorkflowRunResponse> getCreateRunMethod;
    if ((getCreateRunMethod = WorkflowServiceGrpc.getCreateRunMethod) == null) {
      synchronized (WorkflowServiceGrpc.class) {
        if ((getCreateRunMethod = WorkflowServiceGrpc.getCreateRunMethod) == null) {
          WorkflowServiceGrpc.getCreateRunMethod = getCreateRunMethod =
              io.grpc.MethodDescriptor.<talon.v1.Workflows.CreateWorkflowRunRequest, talon.v1.Workflows.WorkflowRunResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateRun"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Workflows.CreateWorkflowRunRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Workflows.WorkflowRunResponse.getDefaultInstance()))
              .setSchemaDescriptor(new WorkflowServiceMethodDescriptorSupplier("CreateRun"))
              .build();
        }
      }
    }
    return getCreateRunMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Workflows.GetWorkflowRunRequest,
      talon.v1.Workflows.WorkflowRunResponse> getGetRunMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetRun",
      requestType = talon.v1.Workflows.GetWorkflowRunRequest.class,
      responseType = talon.v1.Workflows.WorkflowRunResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Workflows.GetWorkflowRunRequest,
      talon.v1.Workflows.WorkflowRunResponse> getGetRunMethod() {
    io.grpc.MethodDescriptor<talon.v1.Workflows.GetWorkflowRunRequest, talon.v1.Workflows.WorkflowRunResponse> getGetRunMethod;
    if ((getGetRunMethod = WorkflowServiceGrpc.getGetRunMethod) == null) {
      synchronized (WorkflowServiceGrpc.class) {
        if ((getGetRunMethod = WorkflowServiceGrpc.getGetRunMethod) == null) {
          WorkflowServiceGrpc.getGetRunMethod = getGetRunMethod =
              io.grpc.MethodDescriptor.<talon.v1.Workflows.GetWorkflowRunRequest, talon.v1.Workflows.WorkflowRunResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetRun"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Workflows.GetWorkflowRunRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Workflows.WorkflowRunResponse.getDefaultInstance()))
              .setSchemaDescriptor(new WorkflowServiceMethodDescriptorSupplier("GetRun"))
              .build();
        }
      }
    }
    return getGetRunMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Workflows.ListWorkflowRunsRequest,
      talon.v1.Workflows.ListWorkflowRunsResponse> getListRunsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListRuns",
      requestType = talon.v1.Workflows.ListWorkflowRunsRequest.class,
      responseType = talon.v1.Workflows.ListWorkflowRunsResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Workflows.ListWorkflowRunsRequest,
      talon.v1.Workflows.ListWorkflowRunsResponse> getListRunsMethod() {
    io.grpc.MethodDescriptor<talon.v1.Workflows.ListWorkflowRunsRequest, talon.v1.Workflows.ListWorkflowRunsResponse> getListRunsMethod;
    if ((getListRunsMethod = WorkflowServiceGrpc.getListRunsMethod) == null) {
      synchronized (WorkflowServiceGrpc.class) {
        if ((getListRunsMethod = WorkflowServiceGrpc.getListRunsMethod) == null) {
          WorkflowServiceGrpc.getListRunsMethod = getListRunsMethod =
              io.grpc.MethodDescriptor.<talon.v1.Workflows.ListWorkflowRunsRequest, talon.v1.Workflows.ListWorkflowRunsResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListRuns"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Workflows.ListWorkflowRunsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Workflows.ListWorkflowRunsResponse.getDefaultInstance()))
              .setSchemaDescriptor(new WorkflowServiceMethodDescriptorSupplier("ListRuns"))
              .build();
        }
      }
    }
    return getListRunsMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Workflows.ResumeWorkflowRunRequest,
      talon.v1.Workflows.WorkflowRunResponse> getResumeRunMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ResumeRun",
      requestType = talon.v1.Workflows.ResumeWorkflowRunRequest.class,
      responseType = talon.v1.Workflows.WorkflowRunResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Workflows.ResumeWorkflowRunRequest,
      talon.v1.Workflows.WorkflowRunResponse> getResumeRunMethod() {
    io.grpc.MethodDescriptor<talon.v1.Workflows.ResumeWorkflowRunRequest, talon.v1.Workflows.WorkflowRunResponse> getResumeRunMethod;
    if ((getResumeRunMethod = WorkflowServiceGrpc.getResumeRunMethod) == null) {
      synchronized (WorkflowServiceGrpc.class) {
        if ((getResumeRunMethod = WorkflowServiceGrpc.getResumeRunMethod) == null) {
          WorkflowServiceGrpc.getResumeRunMethod = getResumeRunMethod =
              io.grpc.MethodDescriptor.<talon.v1.Workflows.ResumeWorkflowRunRequest, talon.v1.Workflows.WorkflowRunResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ResumeRun"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Workflows.ResumeWorkflowRunRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Workflows.WorkflowRunResponse.getDefaultInstance()))
              .setSchemaDescriptor(new WorkflowServiceMethodDescriptorSupplier("ResumeRun"))
              .build();
        }
      }
    }
    return getResumeRunMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Workflows.CancelWorkflowRunRequest,
      talon.v1.Workflows.WorkflowRunResponse> getCancelRunMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CancelRun",
      requestType = talon.v1.Workflows.CancelWorkflowRunRequest.class,
      responseType = talon.v1.Workflows.WorkflowRunResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Workflows.CancelWorkflowRunRequest,
      talon.v1.Workflows.WorkflowRunResponse> getCancelRunMethod() {
    io.grpc.MethodDescriptor<talon.v1.Workflows.CancelWorkflowRunRequest, talon.v1.Workflows.WorkflowRunResponse> getCancelRunMethod;
    if ((getCancelRunMethod = WorkflowServiceGrpc.getCancelRunMethod) == null) {
      synchronized (WorkflowServiceGrpc.class) {
        if ((getCancelRunMethod = WorkflowServiceGrpc.getCancelRunMethod) == null) {
          WorkflowServiceGrpc.getCancelRunMethod = getCancelRunMethod =
              io.grpc.MethodDescriptor.<talon.v1.Workflows.CancelWorkflowRunRequest, talon.v1.Workflows.WorkflowRunResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CancelRun"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Workflows.CancelWorkflowRunRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Workflows.WorkflowRunResponse.getDefaultInstance()))
              .setSchemaDescriptor(new WorkflowServiceMethodDescriptorSupplier("CancelRun"))
              .build();
        }
      }
    }
    return getCancelRunMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Workflows.StreamWorkflowEventsRequest,
      talon.data.Data.WorkflowRunEvent> getStreamEventsMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "StreamEvents",
      requestType = talon.v1.Workflows.StreamWorkflowEventsRequest.class,
      responseType = talon.data.Data.WorkflowRunEvent.class,
      methodType = io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
  public static io.grpc.MethodDescriptor<talon.v1.Workflows.StreamWorkflowEventsRequest,
      talon.data.Data.WorkflowRunEvent> getStreamEventsMethod() {
    io.grpc.MethodDescriptor<talon.v1.Workflows.StreamWorkflowEventsRequest, talon.data.Data.WorkflowRunEvent> getStreamEventsMethod;
    if ((getStreamEventsMethod = WorkflowServiceGrpc.getStreamEventsMethod) == null) {
      synchronized (WorkflowServiceGrpc.class) {
        if ((getStreamEventsMethod = WorkflowServiceGrpc.getStreamEventsMethod) == null) {
          WorkflowServiceGrpc.getStreamEventsMethod = getStreamEventsMethod =
              io.grpc.MethodDescriptor.<talon.v1.Workflows.StreamWorkflowEventsRequest, talon.data.Data.WorkflowRunEvent>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.SERVER_STREAMING)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "StreamEvents"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Workflows.StreamWorkflowEventsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.data.Data.WorkflowRunEvent.getDefaultInstance()))
              .setSchemaDescriptor(new WorkflowServiceMethodDescriptorSupplier("StreamEvents"))
              .build();
        }
      }
    }
    return getStreamEventsMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static WorkflowServiceStub newStub(io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<WorkflowServiceStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<WorkflowServiceStub>() {
        @java.lang.Override
        public WorkflowServiceStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new WorkflowServiceStub(channel, callOptions);
        }
      };
    return WorkflowServiceStub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports all types of calls on the service
   */
  public static WorkflowServiceBlockingV2Stub newBlockingV2Stub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<WorkflowServiceBlockingV2Stub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<WorkflowServiceBlockingV2Stub>() {
        @java.lang.Override
        public WorkflowServiceBlockingV2Stub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new WorkflowServiceBlockingV2Stub(channel, callOptions);
        }
      };
    return WorkflowServiceBlockingV2Stub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static WorkflowServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<WorkflowServiceBlockingStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<WorkflowServiceBlockingStub>() {
        @java.lang.Override
        public WorkflowServiceBlockingStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new WorkflowServiceBlockingStub(channel, callOptions);
        }
      };
    return WorkflowServiceBlockingStub.newStub(factory, channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static WorkflowServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<WorkflowServiceFutureStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<WorkflowServiceFutureStub>() {
        @java.lang.Override
        public WorkflowServiceFutureStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new WorkflowServiceFutureStub(channel, callOptions);
        }
      };
    return WorkflowServiceFutureStub.newStub(factory, channel);
  }

  /**
   */
  public interface AsyncService {

    /**
     */
    default void createRun(talon.v1.Workflows.CreateWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Workflows.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateRunMethod(), responseObserver);
    }

    /**
     */
    default void getRun(talon.v1.Workflows.GetWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Workflows.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetRunMethod(), responseObserver);
    }

    /**
     */
    default void listRuns(talon.v1.Workflows.ListWorkflowRunsRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Workflows.ListWorkflowRunsResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListRunsMethod(), responseObserver);
    }

    /**
     */
    default void resumeRun(talon.v1.Workflows.ResumeWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Workflows.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getResumeRunMethod(), responseObserver);
    }

    /**
     */
    default void cancelRun(talon.v1.Workflows.CancelWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Workflows.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCancelRunMethod(), responseObserver);
    }

    /**
     */
    default void streamEvents(talon.v1.Workflows.StreamWorkflowEventsRequest request,
        io.grpc.stub.StreamObserver<talon.data.Data.WorkflowRunEvent> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getStreamEventsMethod(), responseObserver);
    }
  }

  /**
   * Base class for the server implementation of the service WorkflowService.
   */
  public static abstract class WorkflowServiceImplBase
      implements io.grpc.BindableService, AsyncService {

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return WorkflowServiceGrpc.bindService(this);
    }
  }

  /**
   * A stub to allow clients to do asynchronous rpc calls to service WorkflowService.
   */
  public static final class WorkflowServiceStub
      extends io.grpc.stub.AbstractAsyncStub<WorkflowServiceStub> {
    private WorkflowServiceStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected WorkflowServiceStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new WorkflowServiceStub(channel, callOptions);
    }

    /**
     */
    public void createRun(talon.v1.Workflows.CreateWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Workflows.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateRunMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getRun(talon.v1.Workflows.GetWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Workflows.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetRunMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void listRuns(talon.v1.Workflows.ListWorkflowRunsRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Workflows.ListWorkflowRunsResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListRunsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void resumeRun(talon.v1.Workflows.ResumeWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Workflows.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getResumeRunMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void cancelRun(talon.v1.Workflows.CancelWorkflowRunRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Workflows.WorkflowRunResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCancelRunMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void streamEvents(talon.v1.Workflows.StreamWorkflowEventsRequest request,
        io.grpc.stub.StreamObserver<talon.data.Data.WorkflowRunEvent> responseObserver) {
      io.grpc.stub.ClientCalls.asyncServerStreamingCall(
          getChannel().newCall(getStreamEventsMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   * A stub to allow clients to do synchronous rpc calls to service WorkflowService.
   */
  public static final class WorkflowServiceBlockingV2Stub
      extends io.grpc.stub.AbstractBlockingStub<WorkflowServiceBlockingV2Stub> {
    private WorkflowServiceBlockingV2Stub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected WorkflowServiceBlockingV2Stub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new WorkflowServiceBlockingV2Stub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Workflows.WorkflowRunResponse createRun(talon.v1.Workflows.CreateWorkflowRunRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Workflows.WorkflowRunResponse getRun(talon.v1.Workflows.GetWorkflowRunRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Workflows.ListWorkflowRunsResponse listRuns(talon.v1.Workflows.ListWorkflowRunsRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListRunsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Workflows.WorkflowRunResponse resumeRun(talon.v1.Workflows.ResumeWorkflowRunRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getResumeRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Workflows.WorkflowRunResponse cancelRun(talon.v1.Workflows.CancelWorkflowRunRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCancelRunMethod(), getCallOptions(), request);
    }

    /**
     */
    @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/10918")
    public io.grpc.stub.BlockingClientCall<?, talon.data.Data.WorkflowRunEvent>
        streamEvents(talon.v1.Workflows.StreamWorkflowEventsRequest request) {
      return io.grpc.stub.ClientCalls.blockingV2ServerStreamingCall(
          getChannel(), getStreamEventsMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do limited synchronous rpc calls to service WorkflowService.
   */
  public static final class WorkflowServiceBlockingStub
      extends io.grpc.stub.AbstractBlockingStub<WorkflowServiceBlockingStub> {
    private WorkflowServiceBlockingStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected WorkflowServiceBlockingStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new WorkflowServiceBlockingStub(channel, callOptions);
    }

    /**
     */
    public talon.v1.Workflows.WorkflowRunResponse createRun(talon.v1.Workflows.CreateWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Workflows.WorkflowRunResponse getRun(talon.v1.Workflows.GetWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Workflows.ListWorkflowRunsResponse listRuns(talon.v1.Workflows.ListWorkflowRunsRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListRunsMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Workflows.WorkflowRunResponse resumeRun(talon.v1.Workflows.ResumeWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getResumeRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Workflows.WorkflowRunResponse cancelRun(talon.v1.Workflows.CancelWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCancelRunMethod(), getCallOptions(), request);
    }

    /**
     */
    public java.util.Iterator<talon.data.Data.WorkflowRunEvent> streamEvents(
        talon.v1.Workflows.StreamWorkflowEventsRequest request) {
      return io.grpc.stub.ClientCalls.blockingServerStreamingCall(
          getChannel(), getStreamEventsMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do ListenableFuture-style rpc calls to service WorkflowService.
   */
  public static final class WorkflowServiceFutureStub
      extends io.grpc.stub.AbstractFutureStub<WorkflowServiceFutureStub> {
    private WorkflowServiceFutureStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected WorkflowServiceFutureStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new WorkflowServiceFutureStub(channel, callOptions);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Workflows.WorkflowRunResponse> createRun(
        talon.v1.Workflows.CreateWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateRunMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Workflows.WorkflowRunResponse> getRun(
        talon.v1.Workflows.GetWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetRunMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Workflows.ListWorkflowRunsResponse> listRuns(
        talon.v1.Workflows.ListWorkflowRunsRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListRunsMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Workflows.WorkflowRunResponse> resumeRun(
        talon.v1.Workflows.ResumeWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getResumeRunMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Workflows.WorkflowRunResponse> cancelRun(
        talon.v1.Workflows.CancelWorkflowRunRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCancelRunMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_CREATE_RUN = 0;
  private static final int METHODID_GET_RUN = 1;
  private static final int METHODID_LIST_RUNS = 2;
  private static final int METHODID_RESUME_RUN = 3;
  private static final int METHODID_CANCEL_RUN = 4;
  private static final int METHODID_STREAM_EVENTS = 5;

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
        case METHODID_CREATE_RUN:
          serviceImpl.createRun((talon.v1.Workflows.CreateWorkflowRunRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Workflows.WorkflowRunResponse>) responseObserver);
          break;
        case METHODID_GET_RUN:
          serviceImpl.getRun((talon.v1.Workflows.GetWorkflowRunRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Workflows.WorkflowRunResponse>) responseObserver);
          break;
        case METHODID_LIST_RUNS:
          serviceImpl.listRuns((talon.v1.Workflows.ListWorkflowRunsRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Workflows.ListWorkflowRunsResponse>) responseObserver);
          break;
        case METHODID_RESUME_RUN:
          serviceImpl.resumeRun((talon.v1.Workflows.ResumeWorkflowRunRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Workflows.WorkflowRunResponse>) responseObserver);
          break;
        case METHODID_CANCEL_RUN:
          serviceImpl.cancelRun((talon.v1.Workflows.CancelWorkflowRunRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Workflows.WorkflowRunResponse>) responseObserver);
          break;
        case METHODID_STREAM_EVENTS:
          serviceImpl.streamEvents((talon.v1.Workflows.StreamWorkflowEventsRequest) request,
              (io.grpc.stub.StreamObserver<talon.data.Data.WorkflowRunEvent>) responseObserver);
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
          getCreateRunMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Workflows.CreateWorkflowRunRequest,
              talon.v1.Workflows.WorkflowRunResponse>(
                service, METHODID_CREATE_RUN)))
        .addMethod(
          getGetRunMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Workflows.GetWorkflowRunRequest,
              talon.v1.Workflows.WorkflowRunResponse>(
                service, METHODID_GET_RUN)))
        .addMethod(
          getListRunsMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Workflows.ListWorkflowRunsRequest,
              talon.v1.Workflows.ListWorkflowRunsResponse>(
                service, METHODID_LIST_RUNS)))
        .addMethod(
          getResumeRunMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Workflows.ResumeWorkflowRunRequest,
              talon.v1.Workflows.WorkflowRunResponse>(
                service, METHODID_RESUME_RUN)))
        .addMethod(
          getCancelRunMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Workflows.CancelWorkflowRunRequest,
              talon.v1.Workflows.WorkflowRunResponse>(
                service, METHODID_CANCEL_RUN)))
        .addMethod(
          getStreamEventsMethod(),
          io.grpc.stub.ServerCalls.asyncServerStreamingCall(
            new MethodHandlers<
              talon.v1.Workflows.StreamWorkflowEventsRequest,
              talon.data.Data.WorkflowRunEvent>(
                service, METHODID_STREAM_EVENTS)))
        .build();
  }

  private static abstract class WorkflowServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    WorkflowServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return talon.v1.Workflows.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("WorkflowService");
    }
  }

  private static final class WorkflowServiceFileDescriptorSupplier
      extends WorkflowServiceBaseDescriptorSupplier {
    WorkflowServiceFileDescriptorSupplier() {}
  }

  private static final class WorkflowServiceMethodDescriptorSupplier
      extends WorkflowServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final java.lang.String methodName;

    WorkflowServiceMethodDescriptorSupplier(java.lang.String methodName) {
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
      synchronized (WorkflowServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new WorkflowServiceFileDescriptorSupplier())
              .addMethod(getCreateRunMethod())
              .addMethod(getGetRunMethod())
              .addMethod(getListRunsMethod())
              .addMethod(getResumeRunMethod())
              .addMethod(getCancelRunMethod())
              .addMethod(getStreamEventsMethod())
              .build();
        }
      }
    }
    return result;
  }
}
