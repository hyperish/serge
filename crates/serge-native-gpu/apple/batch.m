#import <Foundation/Foundation.h>
#import <Metal/Metal.h>

@interface SGSceneProgram : NSObject
@property(nonatomic, strong) id<MTLDevice> device;
@property(nonatomic, strong) id<MTLFunction> vertex;
@property(nonatomic, strong) id<MTLFunction> fragment;
@property(nonatomic, strong) NSMutableDictionary<NSNumber *, id<MTLRenderPipelineState>> *pipelines;
@end
@implementation SGSceneProgram
@end

static void reason(char *output, size_t capacity, NSString *message) {
    if (capacity) snprintf(output, capacity, "%s", message.UTF8String ?: "Metal scene error");
}

void *serge_scene_new(void *device, const char *shader, const char *vs, const char *fs,
                      char *error, size_t capacity) {
    @autoreleasepool {
        SGSceneProgram *program = [SGSceneProgram new];
        program.device = (__bridge id<MTLDevice>)device;
        NSError *failure = nil;
        id<MTLLibrary> library = [program.device newLibraryWithSource:@(shader) options:nil error:&failure];
        program.vertex = [library newFunctionWithName:@(vs)];
        program.fragment = [library newFunctionWithName:@(fs)];
        if (!program.vertex || !program.fragment) {
            reason(error, capacity, failure.localizedDescription ?: @"Missing Metal shader entry point");
            return NULL;
        }
        program.pipelines = [NSMutableDictionary dictionary];
        return (void *)CFBridgingRetain(program);
    }
}
void serge_scene_release(void *program) { CFRelease(program); }

static id<MTLRenderPipelineState> pipeline(SGSceneProgram *program, MTLPixelFormat format, NSError **error) {
    id<MTLRenderPipelineState> cached = program.pipelines[@(format)];
    if (cached) return cached;
    MTLRenderPipelineDescriptor *desc = [MTLRenderPipelineDescriptor new];
    desc.label = @"Serge scene";
    desc.vertexFunction = program.vertex;
    desc.fragmentFunction = program.fragment;
    MTLVertexDescriptor *vertex = [MTLVertexDescriptor vertexDescriptor];
    vertex.layouts[1].stride = 64;
    vertex.layouts[1].stepFunction = MTLVertexStepFunctionPerInstance;
    for (NSUInteger i = 0; i < 4; i++) {
        vertex.attributes[i].bufferIndex = 1;
        vertex.attributes[i].offset = i * 16;
        vertex.attributes[i].format = MTLVertexFormatFloat4;
    }
    desc.vertexDescriptor = vertex;
    MTLRenderPipelineColorAttachmentDescriptor *color = desc.colorAttachments[0];
    color.pixelFormat = format;
    color.blendingEnabled = YES;
    color.sourceRGBBlendFactor = color.sourceAlphaBlendFactor = MTLBlendFactorOne;
    color.destinationRGBBlendFactor = color.destinationAlphaBlendFactor = MTLBlendFactorOneMinusSourceAlpha;
    cached = [program.device newRenderPipelineStateWithDescriptor:desc error:error];
    if (cached) program.pipelines[@(format)] = cached;
    return cached;
}

typedef struct { uint32_t index; const void *texture; } SGViewport;

bool serge_scene_draw(void *programPtr, const void *commandPtr, const void *targetPtr, uint32_t format,
                      const void *glyphPtr, const void *pathPtr, const void *bytes, uint32_t count,
                      const SGViewport *slots, size_t slotCount, uint32_t *drawCalls, char *error, size_t capacity) {
    @autoreleasepool {
        SGSceneProgram *program = (__bridge SGSceneProgram *)programPtr;
        id<MTLCommandBuffer> command = (__bridge id<MTLCommandBuffer>)commandPtr;
        id<MTLTexture> target = (__bridge id<MTLTexture>)targetPtr;
        id<MTLTexture> glyphs = (__bridge id<MTLTexture>)glyphPtr;
        id<MTLTexture> paths = (__bridge id<MTLTexture>)pathPtr;
        const MTLPixelFormat formats[] = { MTLPixelFormatRGBA8Unorm, MTLPixelFormatBGRA8Unorm,
                                          MTLPixelFormatRGBA8Unorm_sRGB, MTLPixelFormatBGRA8Unorm_sRGB };
        if (format >= sizeof(formats) / sizeof(formats[0])) return false;
        if (target.pixelFormat != formats[format]) target = [target newTextureViewWithPixelFormat:formats[format]];
        NSError *failure = nil;
        id<MTLRenderPipelineState> state = pipeline(program, formats[format], &failure);
        if (!state || !target) {
            reason(error, capacity, failure.localizedDescription ?: @"Unsupported Metal target view");
            return false;
        }
        // wgpu can create unretained command buffers. Hold each native resource until completion.
        NSMutableArray *inFlight = [NSMutableArray arrayWithObjects:target, glyphs, paths, state, nil];
        const NSUInteger length = (NSUInteger)count * 64;
        id<MTLBuffer> instances = nil;
        if (length > 4096) {
            instances = [program.device newBufferWithBytes:bytes length:length options:MTLResourceStorageModeShared];
            if (!instances) { reason(error, capacity, @"Could not allocate Metal instance buffer"); return false; }
            [inFlight addObject:instances];
        }
        MTLRenderPassDescriptor *pass = [MTLRenderPassDescriptor renderPassDescriptor];
        pass.colorAttachments[0].texture = target;
        pass.colorAttachments[0].loadAction = MTLLoadActionClear;
        pass.colorAttachments[0].clearColor = MTLClearColorMake(0, 0, 0, 0);
        pass.colorAttachments[0].storeAction = MTLStoreActionStore;
        id<MTLRenderCommandEncoder> enc = [command renderCommandEncoderWithDescriptor:pass];
        if (!enc) { reason(error, capacity, @"Could not open Metal render pass"); return false; }
        [enc setRenderPipelineState:state];
        float globals[4] = { (float)target.width, (float)target.height, 0, 0 };
        [enc setVertexBytes:globals length:sizeof(globals) atIndex:0];
        if (instances) [enc setVertexBuffer:instances offset:0 atIndex:1];
        else if (length) [enc setVertexBytes:bytes length:length atIndex:1];
        [enc setFragmentTexture:paths atIndex:1];
        uint32_t start = 0;
        for (size_t i = 0; i <= slotCount; ++i) {
            uint32_t next = i < slotCount ? slots[i].index : count;
            if (next > start) {
                [enc setFragmentTexture:glyphs atIndex:0];
                [enc drawPrimitives:MTLPrimitiveTypeTriangleStrip vertexStart:0 vertexCount:4 instanceCount:next-start baseInstance:start];
                ++*drawCalls;
            }
            if (i < slotCount) {
                id<MTLTexture> tex = (__bridge id<MTLTexture>)slots[i].texture;
                [inFlight addObject:tex];
                [enc setFragmentTexture:tex atIndex:0];
                [enc drawPrimitives:MTLPrimitiveTypeTriangleStrip vertexStart:0 vertexCount:4 instanceCount:1 baseInstance:next];
                ++*drawCalls;
                start = next + 1;
            }
        }
        [enc endEncoding];
        [command addCompletedHandler:^(id<MTLCommandBuffer> _) { (void)[inFlight count]; }];
        return true;
    }
}
