import * as ScrollAreaPrimitive from "@radix-ui/react-scroll-area";
import type { ReactNode } from "react";

type ScrollAreaProps = {
  children: ReactNode;
  className?: string;
  viewportClassName?: string;
};

export function ScrollArea({ children, className, viewportClassName }: ScrollAreaProps) {
  return (
    <ScrollAreaPrimitive.Root className={className}>
      <ScrollAreaPrimitive.Viewport className={viewportClassName}>{children}</ScrollAreaPrimitive.Viewport>
      <ScrollAreaPrimitive.Scrollbar className="flex w-2.5 touch-none select-none bg-transparent p-0.5">
        <ScrollAreaPrimitive.Thumb className="relative flex-1 rounded-full bg-slate-300" />
      </ScrollAreaPrimitive.Scrollbar>
    </ScrollAreaPrimitive.Root>
  );
}
