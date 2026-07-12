package talon.v1;

import static io.grpc.MethodDescriptor.generateFullMethodName;

/**
 */
@io.grpc.stub.annotations.GrpcGenerated
public final class FileServiceGrpc {

  private FileServiceGrpc() {}

  public static final java.lang.String SERVICE_NAME = "talon.v1.FileService";

  // Static method descriptors that strictly reflect the proto.
  private static volatile io.grpc.MethodDescriptor<talon.v1.Files.CreateFileRequest,
      talon.v1.Files.FileResponse> getCreateFileMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "CreateFile",
      requestType = talon.v1.Files.CreateFileRequest.class,
      responseType = talon.v1.Files.FileResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Files.CreateFileRequest,
      talon.v1.Files.FileResponse> getCreateFileMethod() {
    io.grpc.MethodDescriptor<talon.v1.Files.CreateFileRequest, talon.v1.Files.FileResponse> getCreateFileMethod;
    if ((getCreateFileMethod = FileServiceGrpc.getCreateFileMethod) == null) {
      synchronized (FileServiceGrpc.class) {
        if ((getCreateFileMethod = FileServiceGrpc.getCreateFileMethod) == null) {
          FileServiceGrpc.getCreateFileMethod = getCreateFileMethod =
              io.grpc.MethodDescriptor.<talon.v1.Files.CreateFileRequest, talon.v1.Files.FileResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "CreateFile"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.CreateFileRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.FileResponse.getDefaultInstance()))
              .setSchemaDescriptor(new FileServiceMethodDescriptorSupplier("CreateFile"))
              .build();
        }
      }
    }
    return getCreateFileMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Files.ReadFileRequest,
      talon.v1.Files.ReadFileResponse> getReadFileMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ReadFile",
      requestType = talon.v1.Files.ReadFileRequest.class,
      responseType = talon.v1.Files.ReadFileResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Files.ReadFileRequest,
      talon.v1.Files.ReadFileResponse> getReadFileMethod() {
    io.grpc.MethodDescriptor<talon.v1.Files.ReadFileRequest, talon.v1.Files.ReadFileResponse> getReadFileMethod;
    if ((getReadFileMethod = FileServiceGrpc.getReadFileMethod) == null) {
      synchronized (FileServiceGrpc.class) {
        if ((getReadFileMethod = FileServiceGrpc.getReadFileMethod) == null) {
          FileServiceGrpc.getReadFileMethod = getReadFileMethod =
              io.grpc.MethodDescriptor.<talon.v1.Files.ReadFileRequest, talon.v1.Files.ReadFileResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ReadFile"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.ReadFileRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.ReadFileResponse.getDefaultInstance()))
              .setSchemaDescriptor(new FileServiceMethodDescriptorSupplier("ReadFile"))
              .build();
        }
      }
    }
    return getReadFileMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Files.UpdateFileRequest,
      talon.v1.Files.FileResponse> getUpdateFileMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "UpdateFile",
      requestType = talon.v1.Files.UpdateFileRequest.class,
      responseType = talon.v1.Files.FileResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Files.UpdateFileRequest,
      talon.v1.Files.FileResponse> getUpdateFileMethod() {
    io.grpc.MethodDescriptor<talon.v1.Files.UpdateFileRequest, talon.v1.Files.FileResponse> getUpdateFileMethod;
    if ((getUpdateFileMethod = FileServiceGrpc.getUpdateFileMethod) == null) {
      synchronized (FileServiceGrpc.class) {
        if ((getUpdateFileMethod = FileServiceGrpc.getUpdateFileMethod) == null) {
          FileServiceGrpc.getUpdateFileMethod = getUpdateFileMethod =
              io.grpc.MethodDescriptor.<talon.v1.Files.UpdateFileRequest, talon.v1.Files.FileResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "UpdateFile"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.UpdateFileRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.FileResponse.getDefaultInstance()))
              .setSchemaDescriptor(new FileServiceMethodDescriptorSupplier("UpdateFile"))
              .build();
        }
      }
    }
    return getUpdateFileMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Files.GetFileMetadataRequest,
      talon.v1.Files.FileResponse> getGetFileMetadataMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "GetFileMetadata",
      requestType = talon.v1.Files.GetFileMetadataRequest.class,
      responseType = talon.v1.Files.FileResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Files.GetFileMetadataRequest,
      talon.v1.Files.FileResponse> getGetFileMetadataMethod() {
    io.grpc.MethodDescriptor<talon.v1.Files.GetFileMetadataRequest, talon.v1.Files.FileResponse> getGetFileMetadataMethod;
    if ((getGetFileMetadataMethod = FileServiceGrpc.getGetFileMetadataMethod) == null) {
      synchronized (FileServiceGrpc.class) {
        if ((getGetFileMetadataMethod = FileServiceGrpc.getGetFileMetadataMethod) == null) {
          FileServiceGrpc.getGetFileMetadataMethod = getGetFileMetadataMethod =
              io.grpc.MethodDescriptor.<talon.v1.Files.GetFileMetadataRequest, talon.v1.Files.FileResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "GetFileMetadata"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.GetFileMetadataRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.FileResponse.getDefaultInstance()))
              .setSchemaDescriptor(new FileServiceMethodDescriptorSupplier("GetFileMetadata"))
              .build();
        }
      }
    }
    return getGetFileMetadataMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Files.ListFilesRequest,
      talon.v1.Files.ListFilesResponse> getListFilesMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "ListFiles",
      requestType = talon.v1.Files.ListFilesRequest.class,
      responseType = talon.v1.Files.ListFilesResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Files.ListFilesRequest,
      talon.v1.Files.ListFilesResponse> getListFilesMethod() {
    io.grpc.MethodDescriptor<talon.v1.Files.ListFilesRequest, talon.v1.Files.ListFilesResponse> getListFilesMethod;
    if ((getListFilesMethod = FileServiceGrpc.getListFilesMethod) == null) {
      synchronized (FileServiceGrpc.class) {
        if ((getListFilesMethod = FileServiceGrpc.getListFilesMethod) == null) {
          FileServiceGrpc.getListFilesMethod = getListFilesMethod =
              io.grpc.MethodDescriptor.<talon.v1.Files.ListFilesRequest, talon.v1.Files.ListFilesResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "ListFiles"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.ListFilesRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.ListFilesResponse.getDefaultInstance()))
              .setSchemaDescriptor(new FileServiceMethodDescriptorSupplier("ListFiles"))
              .build();
        }
      }
    }
    return getListFilesMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Files.DeleteFileRequest,
      talon.v1.Files.DeleteFileResponse> getDeleteFileMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "DeleteFile",
      requestType = talon.v1.Files.DeleteFileRequest.class,
      responseType = talon.v1.Files.DeleteFileResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Files.DeleteFileRequest,
      talon.v1.Files.DeleteFileResponse> getDeleteFileMethod() {
    io.grpc.MethodDescriptor<talon.v1.Files.DeleteFileRequest, talon.v1.Files.DeleteFileResponse> getDeleteFileMethod;
    if ((getDeleteFileMethod = FileServiceGrpc.getDeleteFileMethod) == null) {
      synchronized (FileServiceGrpc.class) {
        if ((getDeleteFileMethod = FileServiceGrpc.getDeleteFileMethod) == null) {
          FileServiceGrpc.getDeleteFileMethod = getDeleteFileMethod =
              io.grpc.MethodDescriptor.<talon.v1.Files.DeleteFileRequest, talon.v1.Files.DeleteFileResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "DeleteFile"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.DeleteFileRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.DeleteFileResponse.getDefaultInstance()))
              .setSchemaDescriptor(new FileServiceMethodDescriptorSupplier("DeleteFile"))
              .build();
        }
      }
    }
    return getDeleteFileMethod;
  }

  private static volatile io.grpc.MethodDescriptor<talon.v1.Files.PromoteArtifactRequest,
      talon.v1.Files.FileResponse> getPromoteArtifactMethod;

  @io.grpc.stub.annotations.RpcMethod(
      fullMethodName = SERVICE_NAME + '/' + "PromoteArtifact",
      requestType = talon.v1.Files.PromoteArtifactRequest.class,
      responseType = talon.v1.Files.FileResponse.class,
      methodType = io.grpc.MethodDescriptor.MethodType.UNARY)
  public static io.grpc.MethodDescriptor<talon.v1.Files.PromoteArtifactRequest,
      talon.v1.Files.FileResponse> getPromoteArtifactMethod() {
    io.grpc.MethodDescriptor<talon.v1.Files.PromoteArtifactRequest, talon.v1.Files.FileResponse> getPromoteArtifactMethod;
    if ((getPromoteArtifactMethod = FileServiceGrpc.getPromoteArtifactMethod) == null) {
      synchronized (FileServiceGrpc.class) {
        if ((getPromoteArtifactMethod = FileServiceGrpc.getPromoteArtifactMethod) == null) {
          FileServiceGrpc.getPromoteArtifactMethod = getPromoteArtifactMethod =
              io.grpc.MethodDescriptor.<talon.v1.Files.PromoteArtifactRequest, talon.v1.Files.FileResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(SERVICE_NAME, "PromoteArtifact"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.PromoteArtifactRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  talon.v1.Files.FileResponse.getDefaultInstance()))
              .setSchemaDescriptor(new FileServiceMethodDescriptorSupplier("PromoteArtifact"))
              .build();
        }
      }
    }
    return getPromoteArtifactMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static FileServiceStub newStub(io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<FileServiceStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<FileServiceStub>() {
        @java.lang.Override
        public FileServiceStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new FileServiceStub(channel, callOptions);
        }
      };
    return FileServiceStub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports all types of calls on the service
   */
  public static FileServiceBlockingV2Stub newBlockingV2Stub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<FileServiceBlockingV2Stub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<FileServiceBlockingV2Stub>() {
        @java.lang.Override
        public FileServiceBlockingV2Stub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new FileServiceBlockingV2Stub(channel, callOptions);
        }
      };
    return FileServiceBlockingV2Stub.newStub(factory, channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static FileServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<FileServiceBlockingStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<FileServiceBlockingStub>() {
        @java.lang.Override
        public FileServiceBlockingStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new FileServiceBlockingStub(channel, callOptions);
        }
      };
    return FileServiceBlockingStub.newStub(factory, channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static FileServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    io.grpc.stub.AbstractStub.StubFactory<FileServiceFutureStub> factory =
      new io.grpc.stub.AbstractStub.StubFactory<FileServiceFutureStub>() {
        @java.lang.Override
        public FileServiceFutureStub newStub(io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
          return new FileServiceFutureStub(channel, callOptions);
        }
      };
    return FileServiceFutureStub.newStub(factory, channel);
  }

  /**
   */
  public interface AsyncService {

    /**
     * <pre>
     * Read RPCs return signed URLs when the configured object store supports
     * them. Inline content is a small-object fallback and is capped by the
     * gateway.
     * </pre>
     */
    default void createFile(talon.v1.Files.CreateFileRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.FileResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getCreateFileMethod(), responseObserver);
    }

    /**
     */
    default void readFile(talon.v1.Files.ReadFileRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.ReadFileResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getReadFileMethod(), responseObserver);
    }

    /**
     */
    default void updateFile(talon.v1.Files.UpdateFileRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.FileResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getUpdateFileMethod(), responseObserver);
    }

    /**
     */
    default void getFileMetadata(talon.v1.Files.GetFileMetadataRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.FileResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getGetFileMetadataMethod(), responseObserver);
    }

    /**
     * <pre>
     * V1 returns a flat catalog page filtered by File metadata. Directory-style
     * listing and path indexes are intentionally deferred.
     * </pre>
     */
    default void listFiles(talon.v1.Files.ListFilesRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.ListFilesResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getListFilesMethod(), responseObserver);
    }

    /**
     */
    default void deleteFile(talon.v1.Files.DeleteFileRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.DeleteFileResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getDeleteFileMethod(), responseObserver);
    }

    /**
     */
    default void promoteArtifact(talon.v1.Files.PromoteArtifactRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.FileResponse> responseObserver) {
      io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall(getPromoteArtifactMethod(), responseObserver);
    }
  }

  /**
   * Base class for the server implementation of the service FileService.
   */
  public static abstract class FileServiceImplBase
      implements io.grpc.BindableService, AsyncService {

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return FileServiceGrpc.bindService(this);
    }
  }

  /**
   * A stub to allow clients to do asynchronous rpc calls to service FileService.
   */
  public static final class FileServiceStub
      extends io.grpc.stub.AbstractAsyncStub<FileServiceStub> {
    private FileServiceStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected FileServiceStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new FileServiceStub(channel, callOptions);
    }

    /**
     * <pre>
     * Read RPCs return signed URLs when the configured object store supports
     * them. Inline content is a small-object fallback and is capped by the
     * gateway.
     * </pre>
     */
    public void createFile(talon.v1.Files.CreateFileRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.FileResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getCreateFileMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void readFile(talon.v1.Files.ReadFileRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.ReadFileResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getReadFileMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void updateFile(talon.v1.Files.UpdateFileRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.FileResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getUpdateFileMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getFileMetadata(talon.v1.Files.GetFileMetadataRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.FileResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getGetFileMetadataMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     * <pre>
     * V1 returns a flat catalog page filtered by File metadata. Directory-style
     * listing and path indexes are intentionally deferred.
     * </pre>
     */
    public void listFiles(talon.v1.Files.ListFilesRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.ListFilesResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getListFilesMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void deleteFile(talon.v1.Files.DeleteFileRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.DeleteFileResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getDeleteFileMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void promoteArtifact(talon.v1.Files.PromoteArtifactRequest request,
        io.grpc.stub.StreamObserver<talon.v1.Files.FileResponse> responseObserver) {
      io.grpc.stub.ClientCalls.asyncUnaryCall(
          getChannel().newCall(getPromoteArtifactMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   * A stub to allow clients to do synchronous rpc calls to service FileService.
   */
  public static final class FileServiceBlockingV2Stub
      extends io.grpc.stub.AbstractBlockingStub<FileServiceBlockingV2Stub> {
    private FileServiceBlockingV2Stub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected FileServiceBlockingV2Stub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new FileServiceBlockingV2Stub(channel, callOptions);
    }

    /**
     * <pre>
     * Read RPCs return signed URLs when the configured object store supports
     * them. Inline content is a small-object fallback and is capped by the
     * gateway.
     * </pre>
     */
    public talon.v1.Files.FileResponse createFile(talon.v1.Files.CreateFileRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getCreateFileMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.ReadFileResponse readFile(talon.v1.Files.ReadFileRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getReadFileMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.FileResponse updateFile(talon.v1.Files.UpdateFileRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getUpdateFileMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.FileResponse getFileMetadata(talon.v1.Files.GetFileMetadataRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getGetFileMetadataMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * V1 returns a flat catalog page filtered by File metadata. Directory-style
     * listing and path indexes are intentionally deferred.
     * </pre>
     */
    public talon.v1.Files.ListFilesResponse listFiles(talon.v1.Files.ListFilesRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getListFilesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.DeleteFileResponse deleteFile(talon.v1.Files.DeleteFileRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getDeleteFileMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.FileResponse promoteArtifact(talon.v1.Files.PromoteArtifactRequest request) throws io.grpc.StatusException {
      return io.grpc.stub.ClientCalls.blockingV2UnaryCall(
          getChannel(), getPromoteArtifactMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do limited synchronous rpc calls to service FileService.
   */
  public static final class FileServiceBlockingStub
      extends io.grpc.stub.AbstractBlockingStub<FileServiceBlockingStub> {
    private FileServiceBlockingStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected FileServiceBlockingStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new FileServiceBlockingStub(channel, callOptions);
    }

    /**
     * <pre>
     * Read RPCs return signed URLs when the configured object store supports
     * them. Inline content is a small-object fallback and is capped by the
     * gateway.
     * </pre>
     */
    public talon.v1.Files.FileResponse createFile(talon.v1.Files.CreateFileRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getCreateFileMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.ReadFileResponse readFile(talon.v1.Files.ReadFileRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getReadFileMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.FileResponse updateFile(talon.v1.Files.UpdateFileRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getUpdateFileMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.FileResponse getFileMetadata(talon.v1.Files.GetFileMetadataRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getGetFileMetadataMethod(), getCallOptions(), request);
    }

    /**
     * <pre>
     * V1 returns a flat catalog page filtered by File metadata. Directory-style
     * listing and path indexes are intentionally deferred.
     * </pre>
     */
    public talon.v1.Files.ListFilesResponse listFiles(talon.v1.Files.ListFilesRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getListFilesMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.DeleteFileResponse deleteFile(talon.v1.Files.DeleteFileRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getDeleteFileMethod(), getCallOptions(), request);
    }

    /**
     */
    public talon.v1.Files.FileResponse promoteArtifact(talon.v1.Files.PromoteArtifactRequest request) {
      return io.grpc.stub.ClientCalls.blockingUnaryCall(
          getChannel(), getPromoteArtifactMethod(), getCallOptions(), request);
    }
  }

  /**
   * A stub to allow clients to do ListenableFuture-style rpc calls to service FileService.
   */
  public static final class FileServiceFutureStub
      extends io.grpc.stub.AbstractFutureStub<FileServiceFutureStub> {
    private FileServiceFutureStub(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected FileServiceFutureStub build(
        io.grpc.Channel channel, io.grpc.CallOptions callOptions) {
      return new FileServiceFutureStub(channel, callOptions);
    }

    /**
     * <pre>
     * Read RPCs return signed URLs when the configured object store supports
     * them. Inline content is a small-object fallback and is capped by the
     * gateway.
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Files.FileResponse> createFile(
        talon.v1.Files.CreateFileRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getCreateFileMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Files.ReadFileResponse> readFile(
        talon.v1.Files.ReadFileRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getReadFileMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Files.FileResponse> updateFile(
        talon.v1.Files.UpdateFileRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getUpdateFileMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Files.FileResponse> getFileMetadata(
        talon.v1.Files.GetFileMetadataRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getGetFileMetadataMethod(), getCallOptions()), request);
    }

    /**
     * <pre>
     * V1 returns a flat catalog page filtered by File metadata. Directory-style
     * listing and path indexes are intentionally deferred.
     * </pre>
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Files.ListFilesResponse> listFiles(
        talon.v1.Files.ListFilesRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getListFilesMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Files.DeleteFileResponse> deleteFile(
        talon.v1.Files.DeleteFileRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getDeleteFileMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<talon.v1.Files.FileResponse> promoteArtifact(
        talon.v1.Files.PromoteArtifactRequest request) {
      return io.grpc.stub.ClientCalls.futureUnaryCall(
          getChannel().newCall(getPromoteArtifactMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_CREATE_FILE = 0;
  private static final int METHODID_READ_FILE = 1;
  private static final int METHODID_UPDATE_FILE = 2;
  private static final int METHODID_GET_FILE_METADATA = 3;
  private static final int METHODID_LIST_FILES = 4;
  private static final int METHODID_DELETE_FILE = 5;
  private static final int METHODID_PROMOTE_ARTIFACT = 6;

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
        case METHODID_CREATE_FILE:
          serviceImpl.createFile((talon.v1.Files.CreateFileRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Files.FileResponse>) responseObserver);
          break;
        case METHODID_READ_FILE:
          serviceImpl.readFile((talon.v1.Files.ReadFileRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Files.ReadFileResponse>) responseObserver);
          break;
        case METHODID_UPDATE_FILE:
          serviceImpl.updateFile((talon.v1.Files.UpdateFileRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Files.FileResponse>) responseObserver);
          break;
        case METHODID_GET_FILE_METADATA:
          serviceImpl.getFileMetadata((talon.v1.Files.GetFileMetadataRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Files.FileResponse>) responseObserver);
          break;
        case METHODID_LIST_FILES:
          serviceImpl.listFiles((talon.v1.Files.ListFilesRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Files.ListFilesResponse>) responseObserver);
          break;
        case METHODID_DELETE_FILE:
          serviceImpl.deleteFile((talon.v1.Files.DeleteFileRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Files.DeleteFileResponse>) responseObserver);
          break;
        case METHODID_PROMOTE_ARTIFACT:
          serviceImpl.promoteArtifact((talon.v1.Files.PromoteArtifactRequest) request,
              (io.grpc.stub.StreamObserver<talon.v1.Files.FileResponse>) responseObserver);
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
          getCreateFileMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Files.CreateFileRequest,
              talon.v1.Files.FileResponse>(
                service, METHODID_CREATE_FILE)))
        .addMethod(
          getReadFileMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Files.ReadFileRequest,
              talon.v1.Files.ReadFileResponse>(
                service, METHODID_READ_FILE)))
        .addMethod(
          getUpdateFileMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Files.UpdateFileRequest,
              talon.v1.Files.FileResponse>(
                service, METHODID_UPDATE_FILE)))
        .addMethod(
          getGetFileMetadataMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Files.GetFileMetadataRequest,
              talon.v1.Files.FileResponse>(
                service, METHODID_GET_FILE_METADATA)))
        .addMethod(
          getListFilesMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Files.ListFilesRequest,
              talon.v1.Files.ListFilesResponse>(
                service, METHODID_LIST_FILES)))
        .addMethod(
          getDeleteFileMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Files.DeleteFileRequest,
              talon.v1.Files.DeleteFileResponse>(
                service, METHODID_DELETE_FILE)))
        .addMethod(
          getPromoteArtifactMethod(),
          io.grpc.stub.ServerCalls.asyncUnaryCall(
            new MethodHandlers<
              talon.v1.Files.PromoteArtifactRequest,
              talon.v1.Files.FileResponse>(
                service, METHODID_PROMOTE_ARTIFACT)))
        .build();
  }

  private static abstract class FileServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    FileServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return talon.v1.Files.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("FileService");
    }
  }

  private static final class FileServiceFileDescriptorSupplier
      extends FileServiceBaseDescriptorSupplier {
    FileServiceFileDescriptorSupplier() {}
  }

  private static final class FileServiceMethodDescriptorSupplier
      extends FileServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final java.lang.String methodName;

    FileServiceMethodDescriptorSupplier(java.lang.String methodName) {
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
      synchronized (FileServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new FileServiceFileDescriptorSupplier())
              .addMethod(getCreateFileMethod())
              .addMethod(getReadFileMethod())
              .addMethod(getUpdateFileMethod())
              .addMethod(getGetFileMetadataMethod())
              .addMethod(getListFilesMethod())
              .addMethod(getDeleteFileMethod())
              .addMethod(getPromoteArtifactMethod())
              .build();
        }
      }
    }
    return result;
  }
}
