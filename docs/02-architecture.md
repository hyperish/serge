# 02. 아키텍처와 크레이트 구조 (lace)

> 상태: `apps/`·`crates/` 모노레포 및 `hyper` 코어 `ref` 구조는 **확정**, 세부 크레이트 분할은 **제안** · 기준일: 2026-11-17

`lace` 레포는 Cargo 워크스페이스 기반의 모노레포(`apps/`, `crates/`)로 구성합니다(`hyperish/docs`의 `05-development-rules.md` §1).

## 1. 워크스페이스 디렉터리 구조 (제안)

```
lace/
  apps/
    lace-playground/        # 컴포넌트·반응성·SSR 파이프라인 검증 및 쇼케이스 앱
  crates/
    lace/                   # 사용자 진입 크레이트 (단일 의존성 재수출 + prelude)
    lace-core/              # 뷰 노드·템플릿 표현, 마운트 수명주기, 컨텍스트, 서스펜스/리소스
    lace-macros/            # `rsx!`, `#[component]`, 서버 액션 등 proc macro (hyper-parser 참조)
    lace-dom/               # 브라우저 DOM 템플릿 복제(cloneNode)·수술적 패칭·이벤트 위임 엔진
    lace-ssr/               # 스트리밍 HTML SSR 렌더러 및 선택적/아일랜드 Hydration 런타임
    lace-router/            # 타입 안전 URL 라우터 (CSR/SSR 공용, 네비게이션 취소 토큰 연동)
    lace-ionic/             # Ionic 레퍼런스 기반 적응형 웹 UI 컴포넌트·제스처·트랜지션 시스템
    lace-adapter-ring/      # `ring` 백엔드 연동 어댑터 (HTTP/JSON, WebSocket/WebTransport, 서버 함수)
    lace-adapter-webview/   # 웹뷰 컨테이너 브리지 어댑터 (`hera` 등에서 웹뷰가 필요할 때 연결)
```

## 2. 계층 의존 방향과 `hyper` 코어(`ref`) 수직화

```
[ lace-dom / lace-ssr / lace-ionic / lace-adapter-ring / lace-adapter-webview ]
                                     │
                                     ▼ (단방향 의존)
                         [ lace-core / lace-macros ]
                                     │
                                     ▼ (코어 ref)
                      [ hyper (core / state / serde / api) ]
```

- **반응성 코어 공유 (`hyper-state`)**: `lace`가 독자적인 시그널 시스템을 따로 만들지 않고 `hyper-state`를 그대로 참조(`ref`)하여, `lace` 컴포넌트의 반응형 그래프가 `hyper` 코어 모델과 제로 오버헤드로 연결됩니다.
- **내부 포크 + `unit optimization`**:
  - Leptos의 템플릿/하이드레이션 기법과 Dioxus의 `rsx!` 파서/핫리로드 구조를 레퍼런스·포크 대상으로 삼아, `hyper-state` 및 `hyper-serde`에 최적화된 단일 파이프라인으로 `unit optimization`합니다(`hyperish/docs` `05-development-rules.md` §7).
