/** WebSocket message types from the zcgw gateway. */

export type WsIncoming =
  | { type: "clear"; turn_id: string }
  | { type: "delta"; turn_id: string; content: string }
  | { type: "tool_start"; turn_id: string; tool: string; arguments: string }
  | { type: "tool_result"; turn_id: string; tool: string; success: boolean; output: string }
  | {
      type: "done";
      turn_id: string;
      content: string;
      input_tokens: number;
      output_tokens: number;
    }
  | { type: "error"; turn_id: string; message: string }
  | { type: "queued"; turn_id: string; position: number }
  | { type: "turn_start"; turn_id: string; turn_index: number }
  | {
      type: "status";
      turn_id: string;
      busy: boolean;
      current_turn_index: number;
      history_length: number;
    };

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "error";
  content: string;
  /** Progress/thinking text shown before the final answer (e.g. "Thinking..."). */
  thinking?: string;
  toolCalls?: ToolCallInfo[];
}

export interface ToolCallInfo {
  tool: string;
  arguments: string;
  status: "running" | "success" | "fail";
  output?: string;
}
