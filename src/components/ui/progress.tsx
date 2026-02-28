import * as ProgressPrimitive from "@radix-ui/react-progress";
import { cn } from "../../lib/utils";

type ProgressProps = {
  value: number;
  className?: string;
};

export function Progress({ value, className }: ProgressProps) {
  const bounded = Math.max(0, Math.min(100, value));
  return (
    <ProgressPrimitive.Root
      className={cn("relative h-2.5 w-full overflow-hidden rounded-full bg-slate-200", className)}
      value={bounded}
    >
      <ProgressPrimitive.Indicator
        className="h-full w-full bg-gradient-to-r from-sky-500 to-indigo-500 transition-all"
        style={{ transform: `translateX(-${100 - bounded}%)` }}
      />
    </ProgressPrimitive.Root>
  );
}
