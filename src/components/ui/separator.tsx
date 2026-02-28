import * as SeparatorPrimitive from "@radix-ui/react-separator";

export function Separator({ className }: { className?: string }) {
  return <SeparatorPrimitive.Root className={className ?? "h-px bg-slate-200"} />;
}
