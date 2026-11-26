#include <EGL/egl.h>
#include <GLES2/gl2.h>
#include <GLES2/gl2ext.h>
#include <stdbool.h>

static EGLDisplay display;
static EGLContext context;
static EGLSurface surface;
static GLuint texture, program;

static GLuint compile_shader(GLenum type, const char *text) {
    GLuint shader = glCreateShader(type);
    glShaderSource(shader, 1, &text, NULL);
    glCompileShader(shader);
    GLint compiled = 0;
    glGetShaderiv(shader, GL_COMPILE_STATUS, &compiled);
    if (compiled) return shader;
    glDeleteShader(shader);
    return 0;
}

unsigned probe_camera_context(void) {
    display = eglGetDisplay(EGL_DEFAULT_DISPLAY);
    EGLConfig config;
    EGLint count = 0;
    const EGLint attributes[] = { EGL_SURFACE_TYPE, EGL_PBUFFER_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT,
        EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_ALPHA_SIZE, 8, EGL_NONE };
    const EGLint pixels[] = { EGL_WIDTH, 8, EGL_HEIGHT, 8, EGL_NONE };
    const EGLint version[] = { EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE };
    if (!eglInitialize(display, NULL, NULL) || !eglChooseConfig(display, attributes, &config, 1, &count) || !count) return 0;
    surface = eglCreatePbufferSurface(display, config, pixels);
    context = eglCreateContext(display, config, EGL_NO_CONTEXT, version);
    if (!eglMakeCurrent(display, surface, surface, context)) return 0;
    GLuint vertex = compile_shader(GL_VERTEX_SHADER, "attribute vec2 p; varying vec2 uv; uniform mat4 crop;"
        "void main(){gl_Position=vec4(p,0.,1.);uv=(crop*vec4(p*.5+.5,0.,1.)).xy;}");
    GLuint fragment = compile_shader(GL_FRAGMENT_SHADER, "#extension GL_OES_EGL_image_external : require\n"
        "precision mediump float; varying vec2 uv; uniform samplerExternalOES camera;"
        "void main(){gl_FragColor=texture2D(camera,uv);}");
    if (!vertex || !fragment) return 0;
    program = glCreateProgram();
    glAttachShader(program, vertex);
    glAttachShader(program, fragment);
    glBindAttribLocation(program, 0, "p");
    glLinkProgram(program);
    glDeleteShader(vertex);
    glDeleteShader(fragment);
    GLint linked = 0;
    glGetProgramiv(program, GL_LINK_STATUS, &linked);
    if (!linked) return 0;
    glGenTextures(1, &texture);
    glBindTexture(GL_TEXTURE_EXTERNAL_OES, texture);
    glTexParameteri(GL_TEXTURE_EXTERNAL_OES, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_EXTERNAL_OES, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    return glGetError() == GL_NO_ERROR ? texture : 0;
}

bool probe_camera_draw(const float *transform) {
    const float vertices[] = {-1,-1, 1,-1, -1,1, 1,1};
    glViewport(0,0,8,8);
    glClearColor(1,0,1,1);
    glClear(GL_COLOR_BUFFER_BIT);
    glUseProgram(program);
    glUniformMatrix4fv(glGetUniformLocation(program,"crop"),1,GL_FALSE,transform);
    glUniform1i(glGetUniformLocation(program,"camera"),0);
    glBindTexture(GL_TEXTURE_EXTERNAL_OES,texture);
    glVertexAttribPointer(0,2,GL_FLOAT,GL_FALSE,0,vertices);
    glEnableVertexAttribArray(0);
    glDrawArrays(GL_TRIANGLE_STRIP,0,4);
    unsigned char sampled[8*8*4];
    glReadPixels(0,0,8,8,GL_RGBA,GL_UNSIGNED_BYTE,sampled);
    if (glGetError() != GL_NO_ERROR) return false;
    for (unsigned i=0;i<sizeof(sampled);i+=4)
        if (sampled[i+3] && (sampled[i]!=255 || sampled[i+1]!=0 || sampled[i+2]!=255)) return true;
    return false;
}

void probe_camera_finish(void) {
    glDeleteTextures(1,&texture);
    glDeleteProgram(program);
    eglMakeCurrent(display,EGL_NO_SURFACE,EGL_NO_SURFACE,EGL_NO_CONTEXT);
    eglDestroySurface(display,surface);
    eglDestroyContext(display,context);
    eglTerminate(display);
}
