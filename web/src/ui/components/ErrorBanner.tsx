import { Status } from "./ui/index.ts";

// Banner-positioned wrapper around the Status error primitive. The
// outer element owns positioning only (absolute top-left of the graph
// pane); the inner Status renders the warn-coloured box.
export function ErrorBanner({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <div className="absolute top-3 left-3 max-w-[80%] z-10">
      <Status type="error">{message}</Status>
    </div>
  );
}
