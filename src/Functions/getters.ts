import { IGenerics } from "../Interfaces";

function getParams(msg: string, fn?: (str: string[]) => any): any {
    const params = msg.split(' ');
    return fn ? fn(params) : params;
}


function genericCommand(str: string[]): IGenerics.IChangeCommandGeneric {
    if (!str.length) throw { botMessage: 'You need to specify a command!\n\n/command [ARGS]' };
    const command = str.shift()!.toLowerCase().replace('/', '') as IGenerics.implementedCommands;
    const args = [...str];

    return { command, args };
}

function genericDatabaseChangeCommand(str: string[]): IGenerics.IChangeCommand {
    if (!str.length) throw { botMessage: 'You need to specify a command!\n\n/command [TYPE] [ARGS]' };
    const command = str.shift()!.toLowerCase().replace('/', '') as IGenerics.implementedCommands;
    const type = str.shift()!;
    const args = [...str];

    return { command, type, args };
}
export default { getParams, genericDatabaseChangeCommand, genericCommand };