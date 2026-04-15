import { bridgesCli } from './shared.js';
import type { ToolDefinition } from './shared.js';

export const askAgent: ToolDefinition = {
  name: 'ask_agent',
  description: 'Ask a specific agent by node ID through the current Bridges CLI.',
  parameters: {
    type: 'object',
    properties: {
      nodeId: { type: 'string', description: 'Target node ID (kd_...)' },
      query: { type: 'string', description: 'Question to send' },
      projectId: { type: 'string', description: 'Project ID (proj_...)' },
      newSession: {
        type: 'boolean',
        description: 'Start a fresh conversation session instead of continuing the active one',
      },
    },
    required: ['nodeId', 'query', 'projectId'],
  },
  async execute(params) {
    const args = [
      'ask',
      params.nodeId as string,
      params.query as string,
      '--project',
      params.projectId as string,
    ];
    if (params.newSession) args.push('--new-session');
    const output = await bridgesCli(args);
    return { success: true, output };
  },
};

export const askOwner: ToolDefinition = {
  name: 'ask_owner',
  description: 'Legacy helper. Owner-level routing is not supported by the current CLI.',
  parameters: {
    type: 'object',
    properties: {
      owner: { type: 'string', description: 'Owner name' },
      query: { type: 'string', description: 'Question to send' },
      projectId: { type: 'string', description: 'Project ID (proj_...)' },
    },
    required: ['owner', 'query', 'projectId'],
  },
  async execute(params) {
    return {
      error: `Owner-based ask is not supported by the current CLI. Resolve the owner's node ID with \`bridges members --project ${params.projectId as string}\` and use ask_agent.`,
    };
  },
};

export const broadcast: ToolDefinition = {
  name: 'broadcast',
  description: 'Broadcast a message to all project members through the current Bridges CLI.',
  parameters: {
    type: 'object',
    properties: {
      message: { type: 'string', description: 'Message to broadcast' },
      projectId: { type: 'string', description: 'Project ID (proj_...)' },
    },
    required: ['message', 'projectId'],
  },
  async execute(params) {
    const output = await bridgesCli([
      'broadcast',
      params.message as string,
      '--project',
      params.projectId as string,
    ]);
    return { success: true, output };
  },
};

export const tools = [askAgent, askOwner, broadcast];
