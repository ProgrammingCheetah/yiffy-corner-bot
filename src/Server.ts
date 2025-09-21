//#region Imports
import {
    CConf,
    CFS,
    CLogger,
    CDB
} from './Controllers';
import { Login } from 'furaffinity-api';
import { Telegraf } from 'telegraf';
import fnInit from './Functions/init';
import fnAuth from './Functions/auth';
import fnPosts from './Functions/posts';
import fnGetters from './Functions/getters';
import fnAdders from './Functions/adders';
import fnLogger from './Functions/logger';
import { post } from './models/post';
import { queue } from './models/queue';
import { IChangelog, IConfiguration, IContextExtended, IFileSystem, ILogger, IRunFetchOptions, ISendPostOptions } from './Interfaces';
import { Sequelize } from 'sequelize';
import { tag } from './models/tag';
import { create } from 'apisauce';
import { postCreationAttributes } from './models/post';
import { queueCreationAttributes } from './models/queue';
import moment from 'moment';
import cron from 'node-cron';
import { Op } from 'sequelize';
import { ExtraAnimation, ExtraPhoto } from 'telegraf/typings/telegram-types';
import { InlineKeyboardMarkup } from 'telegraf/typings/core/types/typegram';
import filesystem from 'fs';
import path from 'path';
let minutesSinceLastSent = 0;
//#endregion

//#region Starter Variables
const { token, ownerId, commands, name, cookies } = fnInit.GetInitConfiguration(CConf.GetComponent(), CFS.GetComponent(), {
    logger: CLogger.GetComponent({ caller: 'InitConfig' }),
});

if (!(token && commands && ownerId)) {
    throw new Error('Missing configuration');
}

if (cookies) {
    Login(cookies.a, cookies.b);
}

const latestPosts: string[] = [];
const botVersion = '1.2.3';
const startedAt = moment().utcOffset('-06:00').format('YYYY-MM-DD HH:mm:ss');
const bot = new Telegraf<IContextExtended>(token);
const changelog: IChangelog = {
    '1.2.1': {
        'Added': [
            'Added support for custom tags',
            'Added support for clustering',
            'Added support for hidden owner commands'
        ],
        'Removed': [],
        'Fixed': [
            'Fixed a bug where the id of the cluster would skip if the bot had more than 3 instances of itself'
        ]
    },
    '1.2.2': {
        'Added': [],
        'Removed': [],
        'Fixed': ['Bug fixes']
    },
    '1.2.3': {
        'Added': [
            'Added support for better logging',
            'Added support for extended Context in functions',
            'Added support for automatic restarts at 00:00 (-06:00 UTC)',
        ],
        'Removed': [],
        'Fixed': []
    },
    '1.2.4': {
        'Added': [
            'Added support for bot name referencing'
        ],
        'Removed': [],
        'Fixed': [
            'Fixed a bug with the logs printing [Object object]'
        ]
    }
};
//#endregion


//#region Precompilers
// Check if it is a channel
bot.use(async (ctx, next) => {
    if (ctx?.chat?.type === 'channel') return;
    next();
});

bot.command('ping', (ctx) => {
  return ctx.reply('Pong!');
});

bot.command(['time'], (ctx) => {
    const time = moment().utcOffset('-06:00').format('YYYY-MM-DD HH:mm:ss');
    return ctx.reply(`Bot time is ${time}`);
});

// Check if the bot is about to restart
bot.use(async (ctx, next) => {
    const rightNow = moment().utcOffset('-06:00');
    const restartTime = moment().utcOffset('-06:00').endOf('day');

    if (rightNow.add(5, 'minutes').isAfter(restartTime)) {
        return ctx.reply(`Bot is restarting in less than 5 minutes (Bot time ${moment().utcOffset('-06:00').format('YYYY-MM-DD HH:mm:ss')} + 5 mins > ${restartTime}! Try again in a few minutes.`);
    }
    next();
});

// Check if the user has a username
bot.use((ctx, next) => {
    // @ts-ignore
    const username = ctx.message?.from?.username || ctx.update?.callback_query?.from?.username;
    if (!username) {
        ctx.reply('You need to set a username to use this bot!').catch();
        return;
    }
    const id = fnLogger.getID();
    const logger = CLogger.GetComponent({ caller: `${name?.toUpperCase() || ctx.botInfo.username}] [${username.toUpperCase()}`, id });
    ctx.logger = logger;
    ctx.uuid = id;
    ctx.username = username;
    next();

});
//#endregion

//#region General Use
bot.command(['changelog'], (ctx) => {
    ctx.logger.setSubCaller('changelog').info('User requested changelog');

    const changelogString = Object.keys(changelog).map((version) => {
        // @ts-ignore
        const hasAdded = !!changelog[version].Added.length;
        // @ts-ignore
        const hasRemoved = !!changelog[version].Removed.length;
        // @ts-ignore
        const hasFixed = !!changelog[version].Fixed.length;
        // @ts-ignore
        const added = hasAdded ? changelog[version].Added.map((addedString) => '+ ' + addedString).join('\n') : '';
        // @ts-ignore
        const removed = hasRemoved ? changelog[version].Removed.map((removedString) => '- ' + removedString).join('\n') : '';
        // @ts-ignore
        const fixed = hasFixed ? changelog[version].Fixed.map((fixedString) => '~ ' + fixedString).join('\n') : '';
        if (!(hasAdded || hasRemoved || hasFixed)) return `${version}\n * No changes listed`;
        return `${version}\n==========${hasAdded ? '\n' + added : ''}${hasRemoved ? '\n' + removed : ''}${hasFixed ? '\n' + fixed : ''}\n`;
    }).join('\n');
    return ctx.reply(changelogString);
});

bot.command(['version'], (ctx) => {
    ctx.logger.setSubCaller('Version fn').info('Version Command Called');
    return ctx.reply(`Version: v${botVersion}`);
});
//#endregion

//#region Admin Use
bot.use(async (ctx, next) => {
    ctx.logger.setSubCaller('PostFetchCheckpoint');
    const db = CDB.GetComponent(CConf.GetComponent(), CFS.GetComponent(), { logger: ctx.logger });
    try {
      console.log("Hello");
      await fnAuth.AdminCheckpoint(db, ctx.username);
      next();
    } catch {
      
    }
});



function shouldSend() {
    // Generate a random number between 1 and 99
    const randomNumber = Math.floor(Math.random() * 99) + 1;
    const m = 0.000001;
    const calculatedProbability = m * Math.pow((minutesSinceLastSent / 60) * 100, 4);
    return calculatedProbability > randomNumber;
}




async function runPostFetch(options?: IRunFetchOptions): Promise<any[]> {
    options?.logger?.setSubCaller('runPostFetch');
    const id: string = options?.id || fnLogger.getID();
    const caller = options?.username || 'RunPostFetch';
    const dbLogger = CLogger.GetComponent({ caller, id, subCaller: 'GetPostsDatabase' });
    const db: Sequelize = CDB.GetComponent(CConf.GetComponent(), CFS.GetComponent(), { logger: dbLogger });
    const allTags = await db.models.tag.findAll() as tag[];
    const defaultTags: string[] = options?.defaultTagsOverride || allTags.filter((model: tag) => model.type === 'D').map((model: tag) => encodeURIComponent(model.name));
    const forbiddenTags: string[] = [...allTags.filter((model: tag) => model.type === 'F').map((model: tag) => encodeURIComponent(model.name)), ...(options?.forbiddenTagsOverride || [])];

    let foundPosts: any[] = [];
    let tries = 3;
    let previousLength = -1;
    while (true) {
        const serverReply = await fnPosts.getPosts({ defaultTags, forbiddenTags });
        // If the server is not ok or no data was fetch, retry 3 times before giving up
        if (!serverReply.ok || !serverReply.data?.posts) {
            if (tries-- <= 0) {
                options?.logger?.error('Failed to fetch posts after 3 tries');
                throw { botMessage: 'There seems to be an error with e621!' };
            }
            options?.logger?.info('Server returned no data, retrying...');
            continue;
        }
        // If there are not enough posts (75), assume there it isn't a very popular tag and exit
        if (serverReply.data.posts.length < 75) {
            const rawData = fnPosts.removePostsWithTags(serverReply.data.posts, forbiddenTags);
            foundPosts = [...fnPosts.removeDuplicatePosts(foundPosts, rawData)];
            options?.logger?.info('Server returned less than 75 posts, exiting...');
            break;
        }
        const rawData = fnPosts.removePostsWithTags(serverReply.data.posts, forbiddenTags);
        foundPosts = [...fnPosts.removeDuplicatePosts(foundPosts, rawData)];
        if (foundPosts.length === previousLength) {
            options?.logger?.info('Server returned the same amount of posts, exiting...');
            break;
        }
        previousLength = foundPosts.length;
        if (foundPosts.length >= 75) {
            options?.logger?.info('Server returned 75 posts, exiting...');
            break;
        }
        await new Promise((resolve) => {
            options?.logger?.info('Waiting for half a second...');
            setTimeout(resolve, 500);
        });
    }

    while (foundPosts.length > 75) foundPosts.shift();
    options?.logger?.info('Finished getting posts');
    return foundPosts;
}

bot.command(commands.authenticated.count, async (ctx) => {
    try {
        ctx.logger.setSubCaller('Count fn').info('Count Command Called');
        const logger = CLogger.GetComponent({ caller: ctx.username.toUpperCase(), id: ctx.uuid, subCaller: 'Database' });
        const db: Sequelize = CDB.GetComponent(CConf.GetComponent(), CFS.GetComponent(), { logger });
        const count = await db.models.queue.count();
        return ctx.reply(`There are ${count} posts in queue.`);

    } catch (error: any) {
        ctx.logger.error(error);
        return ctx.reply('There was an error with the database!');
    }
});

bot.command(commands.authenticated.get.posts, async (ctx) => {
    ctx.logger.setSubCaller('GetPosts fn').info('GetPosts Command Called');
    const foundPosts = await runPostFetch({ logger: ctx.logger, id: ctx.uuid, username: ctx.username });
    return fnPosts.sendPostsToChat(ctx, foundPosts);

});

bot.command(commands.authenticated.get.byRank, async (ctx) => {
    ctx.logger.setSubCaller('GetByRank fn').info('GetByRank Command Called');
    const defaultTagsOverride: string[] = ["order:rank", "rating:e"];
    const foundPosts = await runPostFetch({ defaultTagsOverride, logger: ctx.logger, id: ctx.uuid, username: ctx.username });
    return fnPosts.sendPostsToChat(ctx, foundPosts);

});
bot.command(commands.authenticated.get.promising, async (ctx) => {
    ctx.logger.setSubCaller('GetPromising fn').info('GetPromising Command Called');
    const defaultTagsOverride: string[] = ["score:>50", "rating:e"];
    const { logger, uuid: id, username } = ctx;
    const foundPosts = await runPostFetch({ defaultTagsOverride, logger, id, username });
    return fnPosts.sendPostsToChat(ctx, foundPosts);
});
bot.command(commands.authenticated.get.withTags, async (ctx) => {
    ctx.logger.setSubCaller('GetWithTags fn').info('GetWithTags Command Called');
    const tagParams = fnGetters.getParams(ctx.message?.text, fnGetters.genericCommand);
    if (!tagParams.args?.length) return ctx.reply('You need to specify at least one tag!');
    const defaultTagsOverride: string[] = (tagParams.args as string[])
        .filter((tag: string) => !tag.startsWith('-'));

    const forbiddenTagsOverride: string[] = (tagParams.args as string[])
        .filter((tag: string) => tag.startsWith('-'))
        .map((tag: string) => tag.slice(1));
    const { logger, uuid: id, username } = ctx;
    const foundPosts = await runPostFetch({ defaultTagsOverride, forbiddenTagsOverride, logger, id, username });
    if (!foundPosts.length) {
        ctx.logger.info(`No posts found`);
        return ctx.reply('No posts found!');
    };
    return fnPosts.sendPostsToChat(ctx, foundPosts);

});


bot.on('callback_query', async (ctx) => {
    // Get the information about the operation
    ctx.logger.setSubCaller('Callback fn').info('Callback Query Called');
    // @ts-ignore
    const data = JSON.parse(ctx.update.callback_query.data);
    try {
        await ctx.deleteMessage();
    } catch (error) {

    }
    if (data.type === 'erase') return;
    else if (data.type === 'send') {
        ctx.logger.info(`Allowing post ${data.id}`);
        const { uuid: id, username: caller } = ctx;
        const db = CDB.GetComponent(CConf.GetComponent(), CFS.GetComponent(), { logger: CLogger.GetComponent({ caller, subCaller: 'CallbackDB', id }) });
        const existingPost = await db.models.post.findOne({ where: { id: data.id } });
        if (existingPost) {
            ctx.logger.error(`Post ${data.id} already exists in the database`);
            return;
        }

        ctx.logger.info(`Getting post ${data.id} data`);
        // Get the API and get the information from the post
        const api = create({
            baseURL: `https://e621.net/post/show/${data.id}.json`,
            headers: {
                Cookie: 'gw=seen',
                'User-Agent': 'PostSelector-ZielAnima/v0.3',
            },
        });

        // Get the post data
        const result = await api.get('');
        if (!result.ok) {
            ctx.logger.error(`Post ${data.id} not found or not accessible`);
            throw { botMessage: 'There seems to be an error with e621!' };
        };
        const { post } = result.data as any;
        const postCreator: postCreationAttributes = {
            post_id: post.id,
            last_updated: moment().format('YYYY-MM-DD'),
        };

        const postModel = await db.models.post.create(postCreator) as post;
        if (!postModel) {
            ctx.logger.error(`Could not create post ${data.id}`);
            throw { botMessage: 'There seems to be an error with the database!' };
        }
        ctx.logger.info(`Created post ${data.id}`);
        const queueCreator: queueCreationAttributes = {
            post_id: postModel.id!,
        };
        const queueModel = await db.models.queue.create(queueCreator) as queue;
        if (!queueModel) {
            ctx.logger.error(`Could not create queue ${data.id}`);
            throw { botMessage: 'There seems to be an error with the database\'s queue!' };
        }
        ctx.logger.info(`Created queue ${data.id}`);
    }
    else if (data.type === 'destroy') {
        ctx.logger.info(`Deleting post ${data.id}`);
        const db = CDB.GetComponent(CConf.GetComponent(), CFS.GetComponent(), { logger: CLogger.GetComponent({ caller: "CallbackQuery" }) });
        const existingPost = await db.models.post.findOne({ where: { post_id: data.id } }) as post;
        if (!existingPost) {
            ctx.logger.error(`Post ${data.id} does not exist in the database`);
            return ctx.reply('No post found!');
        }
        const queuePost = await db.models.queue.findOne({ where: { post_id: existingPost.id } });
        await queuePost?.destroy();
        await existingPost.destroy();
        ctx.logger.info(`Deleted post ${data.id}`);
    }
});
//#endregion

//#region Owner Use
bot.use((ctx, next) => {
    fnAuth.OwnerCheckpoint(ownerId, ctx.message?.from?.id);
    next();
});

bot.command(['setcookiefa'], async (ctx) => {
    const { args } = fnGetters.getParams(ctx.message?.text, fnGetters.genericCommand);
    // Case: No arguments
    if (!args?.length) return ctx.reply('You need to specify a cookie!');
    const [cookie_a, cookie_b] = args;
    // Case: Not enough arguments
    if (!(cookie_a && cookie_b)) return ctx.reply('You need to specify two cookies!');
    const env = CConf.GetComponent().config().util.getEnv('NODE_ENV');
    const fs = CFS.GetComponent();
    const aPromise = fs.writeVaultFile(path.join(env, 'cookie_a.txt'), cookie_a);
    const bPromise = fs.writeVaultFile(path.join(env, 'cookie_b.txt'), cookie_b);

    const [a, b] = await Promise.all([aPromise, bPromise]);

    if (a && b) {
        ctx.reply('Cookies set!');
        Login(cookie_a, cookie_b);
    } else {
        ctx.reply('Could not set cookies!');
    }

});


bot.command(commands.owner.removeFromSaved, async (ctx) => {
    ctx.logger.setSubCaller('RemoveFromSaved fn').info('RemoveFromSaved Command Called');
    const { args } = fnGetters.getParams(ctx.message?.text, fnGetters.genericCommand);
    if (!args?.length) return ctx.reply('You need to specify a post id!');
    const db = CDB.GetComponent(CConf.GetComponent(), CFS.GetComponent(), { logger: CLogger.GetComponent({ caller: "RemoveFromSaved" }) });
    const postModel = await db.models.post.findOne({ where: { post_id: args[0] } }) as post;
    if (!postModel) return ctx.reply('No post found!');
    const api = create({
        baseURL: `https://e621.net/post/show/${postModel.post_id}.json`,
        headers: {
            Cookie: 'gw=seen',
            'User-Agent': 'PostSelectorBotNodev0.2',
        }
    });

    const response = await api.get('');
    if (!response.ok) return ctx.reply('There seems to be an error with e621!');
    const { post } = response.data as any;
    const isAnimated: boolean = ['gif', 'mp4', 'webm'].includes(post.file.ext);
    const replyMarkup: InlineKeyboardMarkup = {
        inline_keyboard: [
            [
                {
                    text: 'Delete',
                    callback_data: JSON.stringify({ id: post.id, type: 'destroy' }),
                }
            ],
            [
                {
                    text: 'Check e621 Src',
                    url: `https://e621.net/post/show/${post.id}`
                },
            ],
            [
                {
                    text: 'Cancel',
                    callback_data: JSON.stringify({ id: post.id, type: 'erase' })
                }
            ]
        ]

    };

    const extra = { reply_markup: replyMarkup };



    if (isAnimated && post.file.ext === 'gif') {
        ctx.replyWithAnimation(post.file.url, extra);
    } else {
        ctx.replyWithPhoto(post.file.url, extra);
    }
});

const allowedTypes = ['admin', 'defaultTag', 'forbiddenTag'];


bot.command(commands.owner.add, async (ctx) => {
    ctx.logger.setSubCaller('Add fn').info('Add Command Called');
    const params = fnGetters.getParams(ctx.message?.text, fnGetters.genericDatabaseChangeCommand);
    // @ts-ignore
    if (!allowedTypes.includes(params.type)) return ctx.reply(`Invalid types! \nAllowed types: ${allowedTypes.toString().replaceAll(',', ', ')}!`);
    const db: Sequelize = CDB.GetComponent(CConf.GetComponent(), CFS.GetComponent(), { logger: CLogger.GetComponent({ caller: "AddCommand" }) });
    const success = await fnAdders.Add(db, params.type, params.args);
    if (!success) return ctx.reply('There seems to be an error with the database!');
    return ctx.reply('Added!');
});

bot.command(commands.owner.remove, async (ctx) => {
    ctx.logger.setSubCaller('Remove fn').info('Remove Command Called');
    const params = fnGetters.getParams(ctx.message?.text, fnGetters.genericDatabaseChangeCommand);
    // @ts-ignore
    if (!allowedTypes.includes(params.type)) return ctx.reply(`Invalid types! \nAllowed types: ${allowedTypes.toString().replaceAll(',', ', ')}!`);
    const db: Sequelize = CDB.GetComponent(CConf.GetComponent(), CFS.GetComponent(), { logger: CLogger.GetComponent({ caller: "RemoveCommand" }) });
    const success = await fnAdders.Remove(db, params.type, params.args);
    if (!success) return ctx.reply('There seems to be an error with the database!');
    return ctx.reply('Removed!');
});
bot.command(commands.authenticated.list, async (ctx) => {
    ctx.logger.setSubCaller('List fn').info('List Command Called');
    const params = fnGetters.getParams(ctx.message?.text, fnGetters.genericDatabaseChangeCommand);
    // @ts-ignore
    if (!allowedTypes.includes(params.type)) return ctx.reply(`Invalid types! \nAllowed types: ${allowedTypes.toString().replaceAll(',', ', ')}!`);
    const db: Sequelize = CDB.GetComponent(CConf.GetComponent(), CFS.GetComponent(), { logger: CLogger.GetComponent({ caller: "ListCommand" }) });
    const list = await fnAdders.List(db, params.type);
    if (!list.length) return ctx.reply('No values!');
    return ctx.reply(params.type + ':\n\n\>' + list.join('\n\>'));

});
bot.command(['instance'], (ctx) => {
    ctx.logger.setSubCaller('Instance fn').info('Instance Command Called');
    return ctx.reply(`Instance: ${startedAt}`);
});

bot.command('sendnext', async (ctx) => {
    ctx.logger.setSubCaller('sendNext').info('Forcing next post...');
    await sendNext({ force: true });
    ctx.reply('Sent!');
});

bot.command('getlogs', async (ctx) => {
    const cf = CConf.GetComponent();
    const logsDir = cf.getConfiguration<string>('logs.dir');
    if (!logsDir) return ctx.reply('No logs dir!');
    ctx.logger.setSubCaller('getLogs').info(`Getting logs from ${logsDir}...`);

    for (const file of filesystem.readdirSync(logsDir)) {
        // Check that the file is not empty
        if (filesystem.statSync(path.join(logsDir, file)).size === 0) {
            ctx.reply(`${file} is empty!`);
            continue;
        };
        const source = filesystem.readFileSync(path.join(logsDir, file));
        if (!source) continue;
        await ctx.replyWithDocument({ source, filename: file + '.txt' });
    }
    return ctx.reply('Done');
});

bot.command('prob', (ctx) => {
    ctx.logger.setSubCaller('prob').info('Prob Command Called');
    return ctx.reply(`${minutesSinceLastSent}: ${shouldSend()}`);
});
bot.command('setMins', async (ctx) => {
    ctx.logger.setSubCaller('setMins').info('SetMins Command Called');
    const params = fnGetters.getParams(ctx.message?.text, fnGetters.genericCommand);
    if (!params.args.length) return ctx.reply('No args!');
    minutesSinceLastSent = params.args[0];
    return ctx.reply(`Set to ${minutesSinceLastSent}!`);
});
//#endregion

//#region Error Handling
bot.catch((err, ctx) => {
    ctx.logger.error(JSON.stringify(err as { botMessage: string; }));
    const error = err as { botMessage: string; };
    if (error.botMessage) ctx.reply(error.botMessage);
    else ctx.reply('There was an error in the bot.');
    return;
});
//#endregion

//#region Cronjob
async function runSendasync(options?: ISendPostOptions) {
    //#region Post Getter
    const cf: IConfiguration = CConf.GetComponent();
    const fs: IFileSystem = CFS.GetComponent();
    const logger = CLogger.GetComponent({ caller: "Cron", id: fnLogger.getID(), subCaller: 'runSendasync' });
    const db: Sequelize = CDB.GetComponent(cf, CFS.GetComponent(), { logger });
    const queueItem: queue = await db.models.queue.findOne({
        include: [{
            model: db.models.post,
            as: 'post',
        }]
    }) as queue;
    const isPartOfQueue = !!queueItem;
    logger.info(isPartOfQueue ? 'Found a post in the queue!' : 'No post found in the queue!');
    const toBeSent = queueItem || {
        post: (await db.models.post.findAll({
            where: {
                last_updated: {
                    [Op.lte]: moment().subtract(20, 'days').format('YYYY-MM-DD')
                }
            },
            limit: 1,
            order: Sequelize.literal('RANDOM()')
        }))[0] as post
    };
    if (!toBeSent) return;
    const api = create({
        baseURL: `https://e621.net/post/show/${toBeSent.post.post_id}.json`,
        headers: {
            Cookie: 'gw=seen',
            'User-Agent': 'PostSelector-ZielAnima/v0.3',
        }
    });

    const response = await api.get('');
    if (!response.ok) return;

    const { post } = response.data as any;
    //#endregion
    const forbiddenTags = (await db.models.tag.findAll({ where: { type: 'F' } }) as tag[]).map(x => x.name);
    if (fnPosts.postContainsTags(post, forbiddenTags)) {
        logger.error('Post contains forbidden tags!');
        if (isPartOfQueue) {
            logger.error(`Removing queue item ${queueItem.id} from queue`);
            if (isPartOfQueue) await db.models.queue.destroy({ where: { id: queueItem.id } });
        }
        logger.error(`Destroying post ${post.id}`);
        await db.models.post.destroy({ where: { post_id: post.id } });

        throw { botMessage: 'Post contains forbidden tags!' };

    }
    if (latestPosts.includes(post.id)) throw { botMessage: 'This post has been sent recently!' };
    if (latestPosts.length >= 100) latestPosts.shift();
    latestPosts.push(post.id);
    //#region Metadata
    const isAnimated: boolean = ['gif', 'mp4', 'webm'].includes(post.file.ext);
    //#endregion

    //#region Voting
    // const upvotes: string = fnPosts.prepareMarkdown('' + post.score.up);
    // const downvotes: string = fnPosts.prepareMarkdown('' + post.score.down);
    // const total: string = fnPosts.prepareMarkdown('' + post.score.total);
    //#endregion

    //#region Artists

    const isNumberRegex: RegExp = /^\d+$/;
    const isRatioRegex: RegExp = /^\d+\:\d+$/;
    const notAllowedInTags = ['(', ')', '.', '-'];
    const artists = fnPosts.prepareMarkdown((post.tags.artist as string[])
        .filter((artist: string) => {
            const redundantTags = ['sound_warning', 'conditional_dnp', 'unknown_artist', 'anonymous_artist'];
            return !redundantTags.includes(artist);
        }
        )
        .map((artist: string) => {
            for (const char of notAllowedInTags) {
                // @ts-ignore
                artist = artist.replaceAll(char, '');
            }
            return '#' + artist;
        })
        .join(' '));
    const meta = fnPosts.prepareMarkdown(post.tags?.meta?.length
        ? (post.tags?.meta as string[])
            .map((tag: string) => {
                if (isNumberRegex.test(tag) || isRatioRegex.test(tag)) return tag;
                for (const char of notAllowedInTags) {
                    // @ts-ignore
                    tag = tag.replaceAll(char, '');
                }
                return '#' + tag;
            }).join(' ')
        : '');

    //#endregion

    //#region Source
    const e6Url = `https://e621.net/post/show/${post.id}`;
    const source: string[] = post?.sources?.length ? [...post.sources] : [e6Url];
    let sources: string = `[e621 Source](${e6Url}) \\| `;
    for (const [index, sourceURL] of source.entries()) {
        const currIndex = index + 1;
        const isLast = source.length === currIndex;
        sources += `[Source ${currIndex}](${sourceURL})${isLast ? "" : " \\|"} `;
    }
    //#endregion

    //#region Telegram Post Creation
    const channelAtDir = cf.getConfiguration<string>('channel.at', { fatal: true })!;
    const channelAt = await fs.readVaultFile(channelAtDir);
    if (!channelAt) throw { botMessage: 'Could not read channel at file!' };
    // There is always going to be at least one source because of the e621 source inclusion
    const caption = `\\[${post.id}\\]${artists ? `\n\nArtists: ${artists}` : ''}${meta ? `\nMeta: ${meta}` : ''}\n\n${sources}\n\n${channelAt}`;
    const extra: ExtraAnimation | ExtraPhoto = { caption, parse_mode: 'MarkdownV2' };
    //#endregion

    //#region Sending
    logger.info('Sending post to channel...');
    if (isAnimated && post.file.ext === 'gif') {
        bot.telegram.sendAnimation(channelAt, post.file.url, extra);
    } else {
        bot.telegram.sendPhoto(channelAt, post.file.url, extra);
    }
    //#endregion

    //#region Deleter
    logger.info('Deleting post from queue...');
    if (isPartOfQueue) db.models.queue.destroy({ where: { id: queueItem.id } });
    //#endregion
}

async function sendNext(options?: ISendPostOptions) {
    while (true) {
        try {
            await runSendasync(options);
            break;
        } catch (err: any) {
            const logger = CLogger.GetComponent({ caller: 'sendNext' });
            logger.error(err.toString());
        }
    }
}

cron.schedule("*/5 * * * *", async () => await sendNext());
//#endregion


bot.launch();
