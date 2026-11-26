#import <AVFoundation/AVFoundation.h>
#import <CoreVideo/CoreVideo.h>
#import <Metal/Metal.h>

void *serge_camera_cache(void *rawDevice) {
    CVMetalTextureCacheRef cache = NULL;
    CVReturn status = CVMetalTextureCacheCreate(NULL, NULL, (__bridge id<MTLDevice>)rawDevice, NULL, &cache);
    return status == kCVReturnSuccess ? (void *)cache : NULL;
}
void serge_camera_release(void *object) { if (object) CFRelease(object); }

@interface SGMetalImage : NSObject
@property(nonatomic, readonly) CVMetalTextureRef view;
- (instancetype)initWithCache:(CVMetalTextureCacheRef)cache pixelBuffer:(CVPixelBufferRef)buffer;
@end
@implementation SGMetalImage {
    CVPixelBufferRef _pixel;
}
- (instancetype)initWithCache:(CVMetalTextureCacheRef)cache pixelBuffer:(CVPixelBufferRef)buffer {
    if (!(self = [super init])) return nil;
    if (!buffer || CVPixelBufferGetPixelFormatType(buffer) != kCVPixelFormatType_32BGRA) return nil;
    CVReturn status = CVMetalTextureCacheCreateTextureFromImage(NULL, cache, buffer, NULL,
        MTLPixelFormatBGRA8Unorm, CVPixelBufferGetWidth(buffer), CVPixelBufferGetHeight(buffer), 0, &_view);
    if (status != kCVReturnSuccess || !CVMetalTextureGetTexture(_view)) return nil;
    _pixel = CVPixelBufferRetain(buffer);
    return self;
}
- (void)dealloc {
    if (_view) CFRelease(_view);
    if (_pixel) CVPixelBufferRelease(_pixel);
}
@end

void *serge_camera_image(void *cache, void *pixel) {
    return (void *)CFBridgingRetain([[SGMetalImage alloc] initWithCache:cache pixelBuffer:pixel]);
}
void *serge_camera_texture(void *image, uint32_t *width, uint32_t *height) {
    id<MTLTexture> tex = CVMetalTextureGetTexture(((__bridge SGMetalImage *)image).view);
    *width = (uint32_t)tex.width;
    *height = (uint32_t)tex.height;
    return (void *)CFBridgingRetain(tex);
}

@interface SGCapture : NSObject<AVCaptureVideoDataOutputSampleBufferDelegate>
- (instancetype)initWithDevice:(AVCaptureDevice *)device error:(NSError **)error;
- (CVPixelBufferRef)take:(int64_t *)time CF_RETURNS_RETAINED;
- (void)close;
@end
@implementation SGCapture {
    AVCaptureSession *_session;
    AVCaptureVideoDataOutput *_output;
    dispatch_queue_t _queue;
    CVPixelBufferRef _latest;
    int64_t _timestamp;
}
- (instancetype)initWithDevice:(AVCaptureDevice *)camera error:(NSError **)error {
    if (!(self = [super init])) return nil;
    AVCaptureDeviceInput *input = [AVCaptureDeviceInput deviceInputWithDevice:camera error:error];
    if (!input) return nil;
    _session = [AVCaptureSession new];
    _output = [AVCaptureVideoDataOutput new];
    _output.alwaysDiscardsLateVideoFrames = YES;
    _output.videoSettings = @{(id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_32BGRA),
                              (id)kCVPixelBufferMetalCompatibilityKey: @YES};
    if (![_session canAddInput:input]) return nil;
    [_session addInput:input];
    if (![_session canAddOutput:_output]) return nil;
    [_session addOutput:_output];
    _queue = dispatch_queue_create("org.hyperish.serge.camera", DISPATCH_QUEUE_SERIAL);
    [_output setSampleBufferDelegate:self queue:_queue];
    [_session startRunning];
    if (!_session.isRunning) { [self close]; return nil; }
    return self;
}
- (void)captureOutput:(AVCaptureOutput *)output didOutputSampleBuffer:(CMSampleBufferRef)sample fromConnection:(AVCaptureConnection *)connection {
    CVPixelBufferRef next = CMSampleBufferGetImageBuffer(sample);
    if (!next) return;
    CMTime time = CMTimeConvertScale(CMSampleBufferGetPresentationTimeStamp(sample), 1000000000, kCMTimeRoundingMethod_Default);
    @synchronized (self) {
        if (_latest) CVPixelBufferRelease(_latest);
        _latest = CVPixelBufferRetain(next);
        _timestamp = CMTIME_IS_NUMERIC(time) ? time.value : 0;
    }
}
- (CVPixelBufferRef)take:(int64_t *)time {
    @synchronized (self) { CVPixelBufferRef image = _latest; _latest = NULL; *time = _timestamp; return image; }
}
- (void)close {
    [_output setSampleBufferDelegate:nil queue:NULL];
    [_session stopRunning];
    dispatch_sync(_queue, ^{});
}
- (void)dealloc { if (_latest) CVPixelBufferRelease(_latest); }
@end

void *serge_camera_open(const char *uid, char *message, size_t capacity) {
    @autoreleasepool {
        if ([AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeVideo] != AVAuthorizationStatusAuthorized) {
            snprintf(message, capacity, "%s", "camera permission is required");
            return NULL;
        }
        AVCaptureDevice *device = uid ? [AVCaptureDevice deviceWithUniqueID:[NSString stringWithUTF8String:uid]] :
            [AVCaptureDevice defaultDeviceWithMediaType:AVMediaTypeVideo];
        NSError *error = nil;
        SGCapture *capture = device ? [[SGCapture alloc] initWithDevice:device error:&error] : nil;
        if (!capture) snprintf(message, capacity, "%s", error.localizedDescription.UTF8String ?: "camera is unavailable");
        return (void *)CFBridgingRetain(capture);
    }
}
void *serge_camera_take(void *capture, int64_t *time) { return (void *)[(__bridge SGCapture *)capture take:time]; }
void serge_camera_stop(void *capture) { [(__bridge SGCapture *)capture close]; }
void serge_camera_permission(void *ctx, void (*done)(void *, bool)) {
    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeVideo completionHandler:^(BOOL granted){done(ctx, granted); }];
}
