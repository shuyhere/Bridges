import { readSharedFile, bridgesCli } from './shared.js';
import type { ToolDefinition } from './shared.js';

export const projectStatus: ToolDefinition = {
  name: 'project_status',
  description: 'Show project overview from the current checkout and coordination server.',
  parameters: {
    type: 'object',
    properties: {
      projectDir: { type: 'string', description: 'Project directory (defaults to cwd)' },
      projectId: { type: 'string', description: 'Project ID (proj_...) for member lookup' },
    },
  },
  async execute(params) {
    const dir = (params.projectDir as string) ?? process.cwd();
    const projectId = params.projectId as string | undefined;

    // Open todos
    const todosContent = await readSharedFile('TODOS.md', dir);
    const openTodos = todosContent
      .split('\n')
      .filter((line) => line.startsWith('- [ ]')).length;
    const doneTodos = todosContent
      .split('\n')
      .filter((line) => line.startsWith('- [x]')).length;

    // Active debates
    const debatesContent = await readSharedFile('DEBATES.md', dir);
    const openDebates = (debatesContent.match(/^## /gm) || []).length;

    let membersOutput = '';
    try {
      if (projectId) {
        membersOutput = await bridgesCli(['members', '--project', projectId]);
      }
    } catch {
      membersOutput = 'Unable to fetch members.';
    }

    return {
      projectDir: dir,
      projectId: projectId ?? null,
      openTodos,
      doneTodos,
      openDebates,
      membersOutput,
    };
  },
};

export const networkStatus: ToolDefinition = {
  name: 'network_status',
  description: 'Legacy helper. Network status probing is not exposed by the current CLI.',
  parameters: {
    type: 'object',
    properties: {
      projectId: { type: 'string', description: 'Project ID (proj_...)' },
    },
  },
  async execute() {
    return {
      error: 'Network status probing is not exposed by the current Bridges CLI.',
    };
  },
};

export const tools = [projectStatus, networkStatus];
