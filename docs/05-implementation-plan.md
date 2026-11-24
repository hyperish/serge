# 05. 구현 계획 (serge)

> 상태: **제안** · 기준일: 2026-11-24

Phase 1(`v0.1.0`)의 작업 순서와 완료 기준입니다. Phase 1에 들어가는 크레이트는 [`04-open-questions.md`](04-open-questions.md)의 Phase 컷이, 크레이트 구조와 책임은 [`02-architecture.md`](02-architecture.md)가 정합니다. 패밀리 차원의 완료 기준은 `hyperish/docs`의 `00-vision.md`에 있습니다.

`serge`는 `hyve`의 `0.1.0-alpha.N` 프리릴리스가 배포된 뒤 그 버전에 의존해 시작합니다(`hyperish/docs` `05-development-rules.md` §5). 7단계의 원격 호출은 `orbic`의 프리릴리스가 있어야 끝까지 검증할 수 있고, `serge` 크레이트는 `orbic`에 의존하지 않으므로 그 검증은 `apps/serge-playground`에서 합니다.

## 1. 출발점

- 현재 코드는 서드파티 의존성이 없는 자리표시자입니다. 크레이트는 `serge`, `serge-macros`와 `apps/serge-playground`뿐이고, 웹 출력은 HTML 문자열, GPU 렌더러는 프리미티브 집계입니다. 고쳐 쓰지 않고 교체합니다.
- 공개 API의 기준은 문서입니다. `crates/serge/tests/mvp.rs`는 문서 예시를 옮긴 테스트로 다시 씁니다.
- 구현에 앞서 할 일:
  1. 포크 대상의 기준 리비전을 다시 확인해 고정하고 기록합니다(`hyperish/docs` `05-development-rules.md` §7).
  2. 워크스페이스를 `02-architecture.md` §1의 크레이트 구조로 재편합니다.
  3. `hyve` 의존(쇼케이스 앱은 `orbic`도)을 프리릴리스의 정확한 버전 요구로 바꾸고, `Cargo.lock`을 커밋하고, CI에 최소 Rust 버전 빌드를 추가합니다.

## 2. 작업 순서

| 단계 | 크레이트 | 산출물 | 앞선 단계 |
|---|---|---|---|
| 1 | `serge-core` | 뷰 트리, 컴포넌트 인자 규칙, 이벤트 흐름, `ScenePrimitive`·`PaintCtx`, `feature()` 조립, `SergeApp` 루트 | 없음 |
| 2 | `serge-macros` | `ui!` 5대 문법과 세부 규칙, `#[component]` | 1 |
| 3 | `serge-style` | 디자인 토큰, 스타일·레이아웃 IR | 1 |
| 4 | `serge-router` | 타입 안전 라우트, 가드, 화면 전환 시 이전 요청 취소 | 1 |
| 5 | `serge-web-dom`, `serge-web-dev` | `tachys` 포크 편입, 템플릿 복제와 패칭, 조건·반복 영역 갱신, 로컬 데브 서버 | 1~4 |
| 6 | `serge-native-layout`, `serge-native-text`, `serge-native-cpu`, `serge-native-gpu`, `serge-platform-desktop` | `taffy`·`parley`/`swash`·`vello_cpu`·`wgpu` 포크 편입(Phase 1의 GPU 경로는 `wgpu` 인스턴스 배칭이며 `vello` 편입은 Phase 2 이후), 창과 입력, GPU → CPU 폴백 정책 | 1~4 |
| 7 | `serge-adapter-client` | 인프로세스 연결, 원격 전송(두 레일, 스키마 불일치 폴백, 자격 증명 부착, 언마운트 시 취소) | 1 |
| 8 | `serge`, `apps/serge-playground`, `hyve-serge` | 재수출과 prelude, 두 타깃을 보여 주는 쇼케이스, `hyve`가 호출하는 도구 단계 | 1~7 |

SSR/SSG, 적응형 웹 UI, 영속 시그널, 데스크톱 시스템 API, 공유 메모리 IPC는 Phase 2이고, 모바일과 위젯 코드젠, 옵트인 웹뷰, 실시간 스트림은 Phase 3입니다.

## 3. 완료 기준

- **문서 예시**: `README.md`의 `CounterCard`와 `01-dx-and-dsl.md` 예시의 `project_feature`·`ProjectEditorCard`·`RealtimeTelemetry`가 Web DOM과 Desktop Native에서 동작합니다. `#[ios::live_activity]` 예시는 Phase 3이라 제외합니다.
- **문법**: 알 수 없는 위젯·프로퍼티·네임스페이스, 빠진 `key`, 깨진 문법에 대한 컴파일 실패 테스트가 있고, 모두 유한한 시간 안에 진단으로 끝납니다. 핸들러 본문에 쓴 일반 Rust 코드가 그대로 컴파일됩니다.
- **반응성**: 마운트한 뒤 시그널을 바꾸면 텍스트, 프로퍼티, `if`/`for` 영역이 갱신됩니다. 웹은 `wasm32` 번들을 헤드리스 브라우저에서 실행해, 네이티브는 CPU 경로의 프레임 버퍼 스냅샷으로 확인합니다. 헤드리스 브라우저 러너는 구현이 고르고 외부 의존성 목록에 기록합니다.
- **렌더러 대칭**: 같은 `ScenePrimitive`를 GPU와 CPU 경로가 모두 그리고(두 경로의 결과는 구현이 정한 허용 오차 안에서 일치), GPU가 없는 CI에서 CPU 경로로 테스트가 통과합니다. 폴백 정책(자동 전환, 명시 지정, 명시한 GPU 실패 시 오류)을 테스트로 확인합니다.
- **비동기**: 핸들러의 비동기 작업이 UI 스레드를 막지 않고, 뷰를 언마운트하면 취소됩니다.
- **원격 호출**: `apps/serge-playground`에서 `#[server]` 호출이 인프로세스 전송과 원격 전송 양쪽으로 동작하고, 두 레일에서 같은 결과와 같은 오류를 내며, 스키마가 어긋나면 표준 레일로 내려갑니다.
- **의존 경계**: 네이티브 크레이트의 의존 그래프에 WebView가 없고, 웹 번들에 네이티브 레이아웃 솔버가 들어가지 않습니다.
- **포크 검증**: 포크본이 원본과 달라진 점과 측정 결과를 남깁니다.
- **CI**: 포맷, 린트, 테스트, 최소 Rust 버전 빌드가 통과하고, 공개 크레이트 전부가 패키징됩니다. 충족하면 `0.1.0-alpha.N` 프리릴리스를 배포합니다.
