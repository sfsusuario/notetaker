/**
 * Recuerda el tamaño y la posición de la ventana entre arranques. Se guarda en
 * los ajustes (localStorage) y se restaura solo si la posición sigue estando
 * dentro de algún monitor conectado (evita ventanas fuera de pantalla al
 * cambiar de monitor).
 */
import {
  availableMonitors,
  getCurrentWindow,
  PhysicalPosition,
  PhysicalSize,
} from "@tauri-apps/api/window";
import { useSettingsStore } from "../stores/useSettingsStore";

const SAVE_DEBOUNCE_MS = 400;

async function isOnScreen(x: number, y: number, w: number): Promise<boolean> {
  try {
    const monitors = await availableMonitors();
    if (monitors.length === 0) return true;
    // Basta con que una parte usable de la barra de título quede visible.
    return monitors.some((m) => {
      const left = m.position.x;
      const top = m.position.y;
      const right = left + m.size.width;
      const bottom = top + m.size.height;
      return x + w > left + 40 && x < right - 40 && y >= top - 8 && y < bottom - 40;
    });
  } catch {
    return false;
  }
}

export async function restoreWindowState(): Promise<void> {
  const { windowBounds, windowMaximized } = useSettingsStore.getState();
  const win = getCurrentWindow();
  try {
    if (windowBounds && windowBounds.w > 300 && windowBounds.h > 300) {
      const { x, y, w, h } = windowBounds;
      await win.setSize(new PhysicalSize(w, h));
      if (await isOnScreen(x, y, w)) {
        await win.setPosition(new PhysicalPosition(x, y));
      } else {
        await win.center();
      }
    }
    if (windowMaximized) await win.maximize();
  } catch (e) {
    console.warn("restoreWindowState", e);
  }
}

export async function watchWindowState(): Promise<() => void> {
  const win = getCurrentWindow();
  let timer: ReturnType<typeof setTimeout> | null = null;

  const persist = async () => {
    try {
      const maximized = await win.isMaximized();
      if (maximized) {
        useSettingsStore.getState().set({ windowMaximized: true });
        return;
      }
      // Minimizada devuelve dimensiones inservibles: no se guardan.
      if (await win.isMinimized()) return;
      const size = await win.innerSize();
      const pos = await win.outerPosition();
      useSettingsStore.getState().set({
        windowMaximized: false,
        windowBounds: { x: pos.x, y: pos.y, w: size.width, h: size.height },
      });
    } catch (e) {
      console.warn("saveWindowState", e);
    }
  };

  const schedule = () => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => void persist(), SAVE_DEBOUNCE_MS);
  };

  const unResize = await win.onResized(schedule);
  const unMove = await win.onMoved(schedule);
  return () => {
    if (timer) clearTimeout(timer);
    unResize();
    unMove();
  };
}
