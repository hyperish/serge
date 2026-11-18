# 02. 아키텍처와 크레이트 구조 (lace)

> 상태: `apps/`·`crates/` 모노레포 및 `hyper` 코어 `ref` 구조는 **확정**, 세부 크레이트 분할은 **제안** · 기준일: 2026-11-17

`lace` 레포는 Cargo 워크스페이스 기반의 모노레포(`apps/`, `crates/`)로 구성합니다(`hyperish/docs`의 `05-development-rules.md` §1).

## 1. 워크스페이스 디렉터리 구조 (제안)

```
lace/
  apps/
    lace-playground/        # 컴포넌트·반응성·DOM 출력 파이프라인 검증 및 쇼케이스 앱
  crates/
    lace/                   # 사용자 진입 크레이트 (단일 의존성 재수출 + prelude)
    lace-core/              # 뷰 노드·템플릿 표현, 마운트 수명주기, 컨텍스트, 서스펜스/리소스
    lace-macros/            # 공통 UI 문법(`hyper-parser`)의 웹/DOM 타깃 전개 proc macro
    lace-dom/               # 모든 DOM/웹 기능 빌더 + 템플릿 복제(`cloneNode`)·수술적 패칭·이벤트 위임 엔진
    lace-ssr/               # 정적/스트리밍 HTML 문자열 생성 및 Hydration 마커 출력 (`ring` 서빙 연동용)
    lace-router/            # 타입 안전 URL 라우터 (History API 바인딩 및 네비게이션 취소 토큰 연동)
    lace-ionic/             # Ionic 레퍼런스 기반 적응형 웹 UI 컴포넌트·제스처·트랜지션 시스템
    lace-dev/               # 최소 CRA 수준의 로컬 개발용 프리뷰·번들·HMR 데브 서버 (`hyper preview` 연동)
    lace-adapter-client/    # 브라우저 측 `hyper-api` 통신 어댑터 (fetch / WebSocket / WebTransport)
```

## 2. 계층 의존 방향과 `hyper` 코어(`ref`) 수직화

```
[ lace-dom / lace-ssr / lace-ionic / lace-dev / lace-adapter-client ]
                                  │
                                  ▼ (단방향 의존)
                      [ lace-core / lace-macros ]
                                  │
                                  ▼ (코어 ref)
               [ hyper (core / state / parser / serde / api) ]
```

- **반응성 코어 및 UI 문법 공유 (`hyper-state`, `hyper-parser`)**: `lace`가 독자적인 시그널이나 별도 UI 문법 파서를 따로 만들지 않고 `hyper` 코어를 그대로 참조(`ref`)하여, `glint`(네이티브)와 동일한 UI 문법을 유지하면서 출력만 DOM으로 내보냅니다.
- **서빙 및 웹뷰 역할 분리**:
  - 로컬 개발 시의 간편한 프리뷰/데브 서빙은 `lace-dev`가 책임집니다.
  - 프로덕션 서빙은 `ring`(`ring-adapter-lace`), 네이티브 웹뷰 호스팅은 `glint`(`glint-adapter-lace-webview`)가 `lace`의 DOM 출력물을 가져가 처리합니다.
- **내부 포크 + 단계적 `unit optimization`**:
  - Leptos의 템플릿/DOM 패칭 기법과 Dioxus/Slint 계열 매크로 구조를 레퍼런스·포크 대상으로 삼아, (1단계) 불필요한 추상화를 제거해 `hyper-state`에 직결하고, (2단계) 핫패스에 대해 심화 개조를 수행합니다(`hyperish/docs` `05-development-rules.md` §7).
