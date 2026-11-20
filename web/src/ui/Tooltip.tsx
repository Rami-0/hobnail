import { Tooltip as SdkTooltip, TooltipContent, TooltipTrigger } from "@ui-sdk/react/tooltip";
import type { ReactElement } from "react";

export function Tooltip({ children, text }: { children: ReactElement; text: string }) {
  return <SdkTooltip>
    <TooltipTrigger render={children} />
    <TooltipContent className="tooltip">{text}</TooltipContent>
  </SdkTooltip>;
}
