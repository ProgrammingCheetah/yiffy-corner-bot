export type implementedCommands = 'add' | 'remove' | 'list';
export type implementedOperations = 'admin' | 'defaultTag' | 'forbiddenTag';

export interface IChangeCommand {
    command: implementedCommands;
    type: string;
    args: string[];
}

export interface IChangeCommandGeneric {
    command: string;
    args: string[];
}

