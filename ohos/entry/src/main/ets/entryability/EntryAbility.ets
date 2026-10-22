import { AbilityConstant, UIAbility, Want } from '@kit.AbilityKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { window } from '@kit.ArkUI';

const DOMAIN: number = 0x0000;
const TAG: string = 'WordCraft';

/**
 * The app's entry: one window, one page. The page owns the XComponent and the native module; this
 * ability only brings the window up (the system's own title bar, as on Windows and Linux).
 */
export default class EntryAbility extends UIAbility {
  onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    hilog.info(DOMAIN, TAG, 'ability created');
    // Another app asked us to open a document (the file manager's "open with", a mail attachment):
    // the page picks it up and opens a working copy of it.
    this.offerDocument(want);
  }

  onNewWant(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    this.offerDocument(want);
  }

  /** Hand a document Uri to the page, which owns the sandbox copies and the input method. */
  private offerDocument(want: Want): void {
    const uri: string = want.uri ?? '';
    if (uri === '') {
      return;
    }
    hilog.info(DOMAIN, TAG, 'asked to open %{public}s', uri);
    AppStorage.setOrCreate<string>('wordcraft.launchUri', uri);
  }

  onWindowStageCreate(windowStage: window.WindowStage): void {
    windowStage.loadContent('pages/Index', (err) => {
      if (err.code) {
        hilog.error(DOMAIN, TAG, 'cannot load the page: %{public}s', JSON.stringify(err));
        return;
      }
      hilog.info(DOMAIN, TAG, 'page loaded');
    });
  }

  onWindowStageDestroy(): void {
    hilog.info(DOMAIN, TAG, 'window stage destroyed');
  }

  onForeground(): void {}

  onBackground(): void {}

  onDestroy(): void {
    hilog.info(DOMAIN, TAG, 'ability destroyed');
  }
}
