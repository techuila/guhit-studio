// Picks a PNG or JPEG from this computer and stores it in the project's
// `files/` folder (`file_store`): the firm logo, a vicinity map. Returns the
// stored file name, or null when the user cancels.
import { ipc } from "../../contract/ipc";

export function pickImage(): Promise<string | null> {
  return new Promise((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/png,image/jpeg";
    input.onchange = () => {
      const file = input.files?.[0];
      if (!file) return resolve(null);
      const reader = new FileReader();
      reader.onerror = () => reject({ code: "io", message: "The image could not be read. Pick a PNG or JPEG file.", element_ids: [] });
      reader.onload = () => {
        ipc
          .fileStore(file.name, String(reader.result))
          .then((r) => resolve(r.file_name))
          .catch(reject);
      };
      reader.readAsDataURL(file);
    };
    // A cancelled picker fires no change event on every browser; nothing waits on it.
    input.click();
  });
}
