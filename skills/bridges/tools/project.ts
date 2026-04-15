import { bridgesCli } from './shared.js';
import type { ToolDefinition } from './shared.js';

export const projectInit: ToolDefinition = {
  name: 'project_create',
  description: 'Create a Bridges project and return the server project ID.',
  parameters: {
    type: 'object',
    properties: {
      name: {
        type: 'string',
        description: 'Project name',
      },
      description: {
        type: 'string',
        description: 'Optional project description',
      },
    },
    required: ['name'],
  },
  async execute(params) {
    const args = ['create', params.name as string];
    if (params.description) args.push('--description', params.description as string);
    const output = await bridgesCli(args);
    return { success: true, output };
  },
};

export const projectInvite: ToolDefinition = {
  name: 'project_invite',
  description: 'Generate an invite token for another agent to join this project.',
  parameters: {
    type: 'object',
    properties: {
      projectId: { type: 'string', description: 'Project ID to invite into (proj_...)' },
    },
    required: ['projectId'],
  },
  async execute(params) {
    const args = ['invite', '--project', params.projectId as string];
    const output = await bridgesCli(args);
    return { success: true, inviteToken: output };
  },
};

export const projectJoin: ToolDefinition = {
  name: 'project_join',
  description: 'Join a project using an invite token.',
  parameters: {
    type: 'object',
    properties: {
      projectId: { type: 'string', description: 'Project ID to join (proj_...)' },
      token: { type: 'string', description: 'Invite token received from another agent' },
    },
    required: ['projectId', 'token'],
  },
  async execute(params) {
    const args = ['join', '--project', params.projectId as string, params.token as string];
    const output = await bridgesCli(args);
    return { success: true, output };
  },
};

export const projectList: ToolDefinition = {
  name: 'project_status',
  description: 'Show current Bridges status, including configured projects.',
  parameters: {
    type: 'object',
    properties: {},
  },
  async execute() {
    const output = await bridgesCli(['status']);
    return { output };
  },
};

export const tools = [projectInit, projectInvite, projectJoin, projectList];
