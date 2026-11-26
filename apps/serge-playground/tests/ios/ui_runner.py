"""Build a native XCTest runner targeting an already installed iPhone simulator app."""
import shutil
import subprocess


def project_for(root, source, udid, results):
    project = root / "ui"
    project.mkdir()
    shutil.copyfile(source, project / "PlatformTests.swift")
    (project / "Host.swift").write_text("import UIKit\n@main final class Host: UIResponder, UIApplicationDelegate {}\n")
    (project / "project.yml").write_text("""name: Platform
options:
  deploymentTarget: {iOS: '16.2'}
targets:
  TestHost:
    type: application
    platform: iOS
    sources: [Host.swift]
    settings:
      base: {PRODUCT_BUNDLE_IDENTIFIER: org.hyperish.verification.host, GENERATE_INFOPLIST_FILE: YES}
  PlatformTests:
    type: bundle.ui-testing
    platform: iOS
    sources: [PlatformTests.swift]
    dependencies: [{target: TestHost}]
    settings:
      base: {PRODUCT_BUNDLE_IDENTIFIER: org.hyperish.verification.tests, GENERATE_INFOPLIST_FILE: YES, TEST_TARGET_NAME: TestHost}
schemes:
  Platform:
    build:
      targets: {TestHost: all, PlatformTests: [test]}
    test:
      targets: [PlatformTests]
""")
    subprocess.run(["xcodegen", "generate"], cwd=project, check=True)
    args = ["xcodebuild", "test", "-project", "Platform.xcodeproj", "-scheme", "Platform",
            "-destination", f"id={udid}", "-parallel-testing-enabled", "NO", "-resultBundlePath", results]
    return project, args
