# Unnamed Project

本项目为agents项目，受VCPToolBox启发而开发，旨在打造一个agent心流的工作体验，实现一个有记忆，情感的agent工作团队

## Part 1 项目技术选型

### 1.技术栈

使用rust + websocket/http + 网页webui(暂时的，主要是协议设计，后期考虑其他前端选型)
其中rust负责发起，处理api请求，后台部分上下文管理（flexible context window），记忆管理，和命令/工具系统，插件系统等核心部分
其中的ui交互，以及通过websocked协议与rust部分通信

### 2.核心架构

rust core通信以及websocket协议设计。

Rust Core架构规划：

1. Flexible Context Window —— Based on Flet
2. Mem Manage
3. Command System ——VCP like

> 关于核心部分中对”Flet“,"VCP like"以及这些的工作机制部分将在后文展开，对于websocked协议部分的规划将在最后一部分简单描述。

## Part 2 Core on Rust

本节讲述本项目最关键的特色部分，也即对agent工作，表现影响最大的部分。可以称之为一部分的驾驭工程(Harness Engineering)

### 1.Flexible Context window

我设定一个基于时间，对话相对轮数，和token消耗量的一个浮动上下文。
这个上下文是主要基于一个含有必要数据的片(Flet)结构来填充的，本质上是一个json结构数据。

#### Flet 数据结构

我用rust表示:

```rust
  struct Flet {
    chracter:String, //标识一下这些数据属于哪个agent
    time:DateTime, //记录时的时间，如2026-05-23-11:42:46
    messages:Vec<Message>,//对话的数据记入
    total_tokens:usize, //对对话数据中的tokens的估算，不需要很精确
  }

  struct Message {
    role:Role,//标准openai的请求中的role,这里我只支持user system assistant，不支持tool,因为我们不使用这个方式调用工具
    reasoning_content:Option<String>,//思考链条回传，一方面保证思考质量，另一方面如今deepseek要求必传,最后也可以支持我们进行思考的归纳总结
    content:Option<String>,//核心内容
  }

  enum Role {
    System,
    User,
    Assistant
  } //注意serde时要处理成小写
```

根据数据结构不难看出，实际上flet中的messages部分完全就是openai格式下的messages部分。
事实上我们对上下文的注入也是采用直接注入这个flet中的messages部分（这里的关键点是有没有可能不用clone来移动messages的数据）

#### Flet 数据的记入

Flet的记入有十分简单的方式，

从程序运行的角度考虑，我们对agent的状态给到一个一个定义，也即working,stop，其中working状态下agent可能处于思考，回复，或者在等待Commands/Tools的返回，而stop状态也即停止状态，等待用户指令，
而Flet也即从一次用户输入(stop)到另一次用户输入(stop)前的所有的用户，模型，Commands/Tools的所有输入/输出。

从用户输入来看，也就是两次用户纯输入(不含有Commands/Tools输出的)之间的输入输出（用户输入取前一次）

例如：(一个询问天气的模拟对话，**注意：格式不是规定的数据结构，只是模拟**)

```text
user : <(<(SYSTEM\n cur_time: 2026-05-23-12:10:24)>)>\n你好！今天天气怎么样？\n<(<(SYSTEM\n[MemoryRecalling]\n 1.用户住在无锡，一般...\n)>)>

assistant: {
  reasoning_content:用户询问天气......\n我要调用工具查询

  content:我现在马上帮你看看你那天气如何，我记得你是在无锡是吧？\n<<<[ToolCall]>>>\n「tool_name」:「「get_weather」」\n「query」:「「无锡 2026年5月23号」」\n<<<[__END__]>>>\n让我等等结果。
}
//这里发生对命令ToolCall的响应，然后处理得到天气信息，通过用户和”软系统注入“告知模型

user: <(<(SYSTEM\n[ToolResponse]\n tool_name:get_weather\n result:今天无锡天气......)>)>
//再次发起api响应

assistant :{
  reasoning_content:得到结果，我来告知用户

  content:我查到，今天天气........
}
//到此就是第一个loop，上面的所有输入输出就成为一个flet的messages

//下一个loop
user: <(<(SYSTEM\n cur_time: 2026-05-23-12:12:20)>)>\n好的知道了，那我应该穿啥衣服合适？

...  
```
>上述的文本中包含的<(<(SYSTEM\n ...)>)>和<<<[Command]>>> ... <<<[__END__]>>>格式是我们规定格式，用于正则匹配，思想和格式皆参考了VCP.重点会在后面Commands/Tools里讲解。

上述模拟体现了我们对flet记录的时机，后面我们考虑对Flet的处理。

获得一个Flet格式，我们第一步处理就是时间，我们使用utc时间得到获得flet这一刻的时间，记入time,
然后采用写好的方法估算其中文本的tokens量，记入total_tokens,
最后给到character表示agent身份。这样我们就得到了一个Flet.

随后将这个Flet写入一个规定的json文件中（文件路径规范后面部分会讲,文件名是time）,同样的后面也通过这一个文件读取这个Flet。
(推荐通过agent state来判断flet记入，也即是通过维护working,stop来执行flet的记入机制)

#### Flet 数据的读取和相对感知时间（LQ_t）

在长期对话中有了大量Flet，管理Flet也成了必要的步骤，实际上这也是我们Flexible Context Window的开始。

先引入一个量，**相对感知时间(LQ_t)**
计算公式：LQ_t = (H - h) * Inf * n,

- H ：即当前时间
- h : 记录时间Flet 中的time
- H-h : 最后精确并换算成时间差（分钟）即可,如：2026-05-23-12:12:20 - 2026-05-23-12:10:24 = 2 （舍弃秒的差异，同时如果有天以上的差异默认直接 H-h = 50000 ，不做复杂换算）
- Inf : 信息量因子，它的计算公式是：Inf= (Flet.total_tokens/LIMIT_TOKENS) 其中的 LIMIT_TOKENS是由用户在配置文件中决定的（这个配置文件后面部分会有，这个值默认是300000）
- n : 为当前这个flet在列表flets(已经按照了time，由时间近到远排序)中的索引

接下来讲讲如何筛选flet并填入context,以及一些和Mem Manager有关的事项：

1. 我们先对现有的flet目录遍历取出所有的flet文件，集合进入flets:Vec<Flet>,然后对flets按照时间从晚到早进行排序（也即是离现在越早索引越小），
2. 然后遍历计算每一个flet的LQ_t,当LQ_t < 45则可以移入api请求的messages,也即注入了上下文（这一步有两个难点，一是能不能做到不clone,第二是不能把顺序注入错了，时间晚的应该在messages的后面，早的在前面）
3. 对于LQ_t>45的部分我们发起另一个api请求，使用用户配置中指定的Mem_model来对这些flets进行总结，并注入记忆数据库，从此归Mem Manage处理，在确保已经写入数据库后，对这些flets文件进行删除。（这一步触发Mem 中的store机制，详细规划在Mem Manager部分讲解）
4. 除上述之外，我们同时要临时记入一下索引为1,也即是n=1的那个flet的LQ_t_1,当所有flet遍历结束，我们对这个临时的记入的LQ_t_1进行判断，当LQ_t_1 > 1则用户对用户的输入进行记忆查询（也即启动Mem 中的query机制，后面会讲解）


至此，Flexible Context Window部分完结！！！

### 2. Mem Manage

对于agent的记忆结构设置我们主要采用RPG向量化的处理，其中只涉及一些简单的数学计算，参考了VCP以及其它的一些知识库，记忆库的设置。

首先需要说明的是我们的记忆管理基于数据结构Mem,用rust表示如下:

```rust
  struct Mem {
    mem_id : String, //这个mem的唯一表示id,可用uuid生成
    class_id: String, // 这个是mem类别的id,每个类别都是唯一的id,来源后面的讲到store时会说明
    class_name:String,//与class_id匹配的名字
    content:String,//记忆内容
    time:DateTime,// 时间记录，只需要精确到天即可，后面计算也是只用换算到天
    base_score:usize,//基础得分，是后面计算中的重要影响量
  }
```

有了相关数据结构的定义，我们接着讨论下面的具体流程，主要分为 store 和 query 两个阶段

#### store

本阶段是对从Flexible Context Windows中被筛选出来的，LQ_t > 45的flet,理想情况是一次只有一个flet,实际上通常到这一步可能同时存在多个flet,也即我们需要对多个flet进行区分存入。
为实现这一点，以及节省成本，我们通过对flets:Vec<Flet>进行遍历，取出每个flet的messages,并将这些内容包装成一个统一的文本内容（尽可能保留messages中的格式），结果文本称pre_mem_text:String
然后通过拼接一段给定的提示词，对配置中规定的Mem_model发起一次api请求，要求其对这些flet进行总结，
提示词样式(还需要精修)：
```text
  你是一个对话内容总结助手，我接下来会在<Chat>标签内给你提供一些user/assistant的对话记录（几个flet），请你对这些对话做出几点简短总结，要求：
  1.每点总结请遵守一下格式<Mem>\n\\class_id: ... \n\\class_name:...\n\\content:...\n\\time:...</Mem>
  2.对于content，用语风格保持和assistant一致，包括对用户的称呼和口头禅以及尽可能模范语气，内容请参考：时间+事件/事实+结果/效果+用户反馈+工具使用和结果+思考链思路总结。长度不要超过250字
  3.对于class_id,和class_name是你对这个总结的标签的对应id和name,你目前可用的标签的id,name对应:{{mem_classes}}
  4.对于time参考对话中的user 中的cur_time内容。
  5.一个Flet至少要做一个总结（及输出一个完整的<Mem>结构）

  对于对话的文本，做一下说明：
  1.user/assistant后面跟着的就是对应的内容。
  2.user中包含一些特殊格式的内容，例如 <(<(SYSTEM\n ...)>)> 这个是系统的注入，其中可能包含有cur_time，对话发起时间，[ToolResponse]工具执行结果，[MemoryRecalling]assistant记忆注入；在这样的格式外的内容才是用户输入，如果没有则说明是系统发起的请求没有用户输入，请你加以区分。
  3.assistant中包含思考(reasoning_content)和正文(content)两个部分，其中可能涉及类似<<<[Command]>>>「key」:「「value」」<<<[__END__]>>>的格式，这是assistant的工具调用部分，格式以外才是assistant的正常输出，请你加以区分。

  对于标签，除了你可用的这些标签以外如果你需要添加一些新标签，请在输出中包含如下结构：
  <NewClass>class_id->class_name</NewClass>,例如：<NewClass>100->Anime</NewClass>
  
  以下是对话记录：
  <Chat>\n{{pre_mem_text}}\n</Chat>
```
以上便是总结的提示词（初稿），值得注意的是其中使用了一种标记`{{mark}}`，这一标记也是全系统的文本正则替换的一部分（后面部分讲述），
现在我们来讲讲目前出现的两个mark,

1. `{{mem_classes}}`出现这一标签时，我们读取规范下的一个文件（mem_classes.txt）并将内容通过正则替换掉，文件中包含的内容如: `001->facts\n002->happy\n003->programing\n004->bio_kownledge`，也就是一个class_id.class_name的表，用来提供给模型的参考，以及后续使用。
2. `{{pre_mem_text}}`出现这一个标签是，我们对前文说过的flets中的每个messages给统一封装，格式如下：
```text
Flet 0
  user :
    message user
  /user
  assistant :
    reasoning_content:
      message assistant reasoning content
    /reasoning_content
    content :
      message assistant content
    /content
  /assistant
/Flet 0
Flet 1
  ...
  
```

我们捕获到Mem_model的输出之后，即通过正则匹配得到<Mem> ... </Mem>内容，对其中的class_id,class_name,time,content进行匹配处理，以base_score = 0.3填入Mem 。
之后接着给配置中的Embed_model发起api请求，给每个Mem的content进行向量化，之后将每个Mem写入数据库，{{character}}_mem.db 。（character为记忆所属的agent角色名字）。
此外，也识别模型输出中的<NewClass>标签，将新的标签追加写入mem_classes.txt的新一行。（注意可能不止一个新标签）。
最后，前面流程都正常结束之后，对flets原始的那些flet的文件进行删除（删除必须发生在这一步，保证如果程序中途退出也可以在下次将记忆存入而不是彻底丢失）。

>以上store环节的所有部分在触发之后全部异步执行，确保不会影响主线程用户和agent的交流。

#### query

这个阶段负责查询，实际上就是记忆设计的核心部分的记忆召回和清理机制。

同样的我们引入两个公式：

1. 查询公式：
    SF = (1 + e^(-d/30)) * (Re +1) * Base
2. 遗忘判断公式 :
    SFD = (1 + e^(-d/30)) * Base

两个公式中：
- d :表示时间差，单位是天。也即 d = cur_time - mem.time （忽略时与分，换算成天），用来标识时间差距。
- Re :相关性，通过计算用户的输入的向量进行查询计算。
- Base : mem.base_score，即基础分数，用来标识被召回次数。

好的，我们现在有了理论，接下来讨论如何召回，和清理。

工作流程：
1. 当query被触发时，用户输入被embed_model向量化，让后通过向量计算相关新，得到所有Re > 0.5 为一个re_mems:Vec<Mem>，re_mems也即预召回的记忆。
2. 对re_mems中的每个mem进行遍历，计算每个mem的SF，将SF > 0.95 的 top 3 移入新的sf_mems:Vec<Mem>,sf_mems也即是成功召回的记忆，此时注入给新flet的user中，格式是：<(<(SYSTEM\n[MemoryRecalling]\n{{memory_recalling}}\n)>)>
3. 对sf_mems中的mem的base_score “召回奖励”，这个 “奖励是分段的” 当base_score < 0.6, base_score * 1.03；当base_score > 0.6时, base_score * 1.01。
4. 对re_mems中的mem的base_score “预召回奖励”，这个奖励是一致的，都是 base_score * 1.01 .注意此时，sf_mems中的元素应该不再re_mems中了。
5. 对sf_mems中的SF最大的那一个mem启动**记忆联想机制**，这个机制后面讲。
6. 对未处于re_mems,sf_mems中的剩下的mem,也即未召回记忆，给一个分段的惩罚。当base_score > 0.1 ,base_score * 0.98, 当base_score < 0.1时 base_score * 0.99。对base_score < 0.05的mem执行**遗忘判断**。

>对于奖励，或者说base_score有一个上限数值，也即base_score_max = 1.3,基础分不可超过此数值，避免权重过高

##### 记忆联想机制：

本机制确保一定的记忆发散,无关相关性，只使用SDF作为判断依据，同时也不是一定触发，而是概率触发，这是为了限制发散，一定程度避免污染。

一个初步判断，需要sf_mems中的top 1的class_name不是“FORGETED”，如是则不发生联想。

对sf_mems中的top 1的SF数值进行记录，为SF_max
使用随机数生成，获得一个0到1的随机小数，random ,
当random < (SF_max / 4)时，发生联想：
1. 获取标签class_id,检索出所有同id的mem,去除sf_mems中已经包含的得到len_mems:<Mem>
2. 对所有的len_mems中的mems,额外给到一个“联想奖励”，不分段，都是 base_score * 1.01
3. 对每一个len_mems计算SFD，选出top 3注入给下一个flet的user中，格式如上文中的第一次注入。

##### 遗忘判断：

本机制用于判断一些长期低召回的记忆的处理，目的是清理数据空间，同时尽可能避免出现其实重要的事件被彻底遗忘。

当base_score < 0.05后，进入遗忘判断：
1. 收集base_score < 0.05的mem,进入low_mems:Vec<Mem>
2. 遍历low_mems中的mem,求得SFD,将SFD < 0.05的移入 forget_mems:Vec<Mem>
3. 当forget_mems.len() >= 5时，对其中的每一个mem的content拼接成一个forget_text,然后将这个text触发store,对这些mems压缩成一个mem,这个mem的时间调整为当前时间，base_score = 0.2，class_id = “000”, class_name = "FORGETED"重新注入回记忆数据库。
4. 清除掉forget_mems中的所有mem

> 注意，上述的第3.中对store的触发，mem_model的提示词需要换为：(采用兼容原有正则的输出格式)
> 你要负责整理一些记忆信息，我会给你一段文本，来自一个agent的记忆，你需要对这段文本中的关键信息进行提取，要求：1.需要提取的是一些事实性/知识性的内容。2.不允许随便添加任何不存在的内容。3.输出的格式是<Mem>class_name: ...\n\\class_id:...\n\\content:...\n\\time:...\n</Mem>。4.class_name写FORGETED,class_id写000,time可以留空。5.content就是你总结/提取的内容，保持和文本内容相似的风格。\n文本如下：{{forget_text}}

##### query 的注入和触发

**注意**：query的所有步骤也应该是异步执行，发起注入时使用线程通信手段。

1. 触发机制：
实际上我们在flexible context window部分中提到了一个触发机制，也即是LQ_t_1 > 1时触发，这是出于将记忆查询限定在一个合适的频率内，避免过高导致产生大量无意义的token消耗。
另外还有一个机制也会触发query那就是flets.len() < 2时也会触发，这通常意味着，一个新的话题的出现或者是，已经较长时间没有与agents进行对话了，或者在一个flet内agent进行了长时间的自主工作。

2. 注入机制：

我们实际上会触发一次query，同时联想记忆的部分和主要记忆是分批次注入的

模拟一下对话场景:
```text
...
Flet 1  //这里使得Flet 1的LQ_t_1 >1
//下面是Flet 0 的部分
user: ... //记入为user_0_message
assistant: ...
//下面就是当前要发起的
user: user_-1_message + {{sf_mems}} //这里在用户提交了请求之后，将user_-1_message给到后端并触发query，然后首先得到了线程通信得到的sf_mems(已经化为了规定格式),在这里之间注入，后发送api post.
assistant: ...
... // 这个过程中会得到 len_mems先不注入，而是临时保存，后面注入。
//下面是未来要发起的
user: user_-2_message + {{len_mems}} //这里用户发起请求后，我们将得到的len_mems和用户的user_-2_message拼接起来发送api post
```

至此，Mem Manage部分完结！！！

### 3.Command System

本节讲述一个由VCP启发，并且加入了我自己的理解的一个基于对模型输出的文本内容正则匹配的命令机制。
这个机制一方面是为了解决两个现有的tool_call,function的问题：

1. 解决工具调用带来的模型输出的中断，导致效率下降，本系统可以支持模型输出的同时后端异步执行已经触发的命令调用，并在后一次统一发送命令调用结果。
2. 解决模型对工具调用的理解缺乏，这个系统中命令调用和返回都是在assistant/user的同级，而且使用特殊格式有利于模型对命令调用的理解，甚至探索性开发使用（VCP提及的优点）。

另一方面是尝试让agents像 “人类” 一样工作，这一部分会在后言中提及。

#### 语法

先讲述一下规定的标准语法:
```text
  <<<[ExampleCommand]>>>
  「key_1」:「「value_1」」
  「key_2」:「「value_2」」
  ---
  「other_key_1」:「「other_value_1」」
  「other_key_2」:「「other_value_2」」
  <<<[__END__]>>>
```
其中为了加强模型输出的鲁棒性，以及提供匹配的准确性，对正则匹配时做一下规定:

1. ExampleCommand的匹配，我们先对<<<[...]>>>中的内容进行提取，然后消除特殊字符，例如空格，_,- ,最后全部大写化。也即对于，Example_Command,example_command,EXAMPLECOMMAND,ExampleCommand,exampleCommand,最后识别到的都是EXAMPLECOMMAND，但我们提示词中仅告诉agent使用ExampleCommand这一个标准写法。
2. key，value不处理，只进行严格匹配。这意味着value中可以接受一个长的，复杂的字符串，这无疑是对命令调用的极大的开发。
3. 对于截止符<<<[__END__]>>>,我们对其的定义是标识一个命令的末尾但不是严格的匹配符号。这意味着，对一个命令及其参数的匹配从识别到<<<[Command]>>>就开始了，直到遇到下一个<<<[Command]>>>或者是<<<[__END__]>>>结束，亦或者是模型输出结束。但我们在给agents的提示词中仅告知模型<<<[Command]>>>...<<<[__END__]>>>这一标准写法。
4. 对于分隔符`---`,本质上和一个新的命令块一样，是一种简化写法。鼓励模型在同时执行多个相同命令时使用这个写法来简化输出。分隔符分割开的命令是多个参数不同的相同命令，其解析时的等级是相同的。

值得注意的是每个命令当命令块完结也即是对于所有的key,value匹配完，遇到了下一个命令块，或者是<<<[__END__]>>>截止符或分隔符`---`，则无论模型是否仍在输出，都将在后端异步执行命令，且 **所有的命令之间也是异步执行的**。
基础语法就这些，接下来讲解一下支持的命令以及它们的工作系统：

#### ToolCall

本工具给agent提供工具支持，值得注意的是我们这里的工具指的通常是用户自己封装的外部程序和脚本，例如 example.py,
我们的工具调用命令ToolCall被识别之后会根据用户配置文件中的工具定义中的命令执行外部程序，通过stdout获取输出。

本工具支持的参数有tool_name以及对应工具的参数。
解析之后的数据结构用rust表示：
```rust
struct ToolCall {
  tool_name:String, //对应调用时的tool_name
  args:Hashmap<String,String> // 对应除了tool_name以外的其他参数，这里全部解析为hashmap,不做进一步解析
}
```
值得注意的是对于args,也就是工具调用时的支持参数我们内部不做过度解析，只是最后将Hashmap序列化为json格式的字符串作为命令行参数一起传给执行程序或脚本，由对应的工具自行解析参数，
这一目的是减少内部的了解析参数的胶水代码，以及配置书写的复杂性，而对于工具自身也只需要解析一段传入的json文本。

工作流程示例:(以web_search，shell为例)

1. 给出配置文件中的工具定义:(这个语法上不一定符合kdl格式，只是一个示例)
```kdl
  tools {
    "web_search" description="联网搜索，支持参数query,用于查询网络信息，另支持count,用于筛选数量" {"python" "web_search.py"}
    "shell" description="shell 工具，用于执行shell命令,支持参数expresion" {"./safe_shell"} //这里的safe_shell是做过特殊封装的安全shell执行程序
  }
```

2. 调用示例：
```text
  <<<[ToolCall]>>>
  「tool_name」:「「web_search」」
  「query」:「「helix editor 2026」」
  「count」:「「25」」
  ---
  「tool_name」:「「shell」」
  「expression」:「「ls -la」」
  <<<__END__>>>
```
这里涉及一个额外的语法，即命令块中的`---`,这是分隔符，本质上和在写一个ToolCall命令块是一样的效果，**这一点解析时需要格外注意**
同样的，对于使用分隔符分割的不同命令是**异步同时执行**的。

3. 解析后异步执行：(等效的sh命令)

```sh
python web_search.py "{\"query\":\"helix editor 2026\",\"count\":\"25\"}"  
```

```sh
./safe_shell "{\"expression\":\"ls -la\"}"
```
后面由工具自行对json格式进行解析，并执行对应的命令，最后输出。
而我们通过stdout/stderr得到对应输出，封装格式为:
```rust
  struct ToolResponse{
    tool_name:String,//对应调用的tool_name
    result:String,//工具输出
  }
```
得到输出之后等待其他命令/工具执行完成，在全部命令和工具执行完成之后，将内容通过user中的系统软注入发出

4. 注入工具返回结果:

```text
user: <(<(SYSTEM
[ToolResponse]
tool_name:web_search
result: ...
---
tool_name:shell
result: ...

// 下面是其他命令的执行结果
[OtherCommandResponse]
)>)>  
```

#### AgentCall

本命令是的作用是对任意的agent发起一个任务，这一个命令的内部执行和其他的命令有一个本质的区别，在于它可能基于时间进行唤醒。
本命令是一个多功能命令，既可以用于发起一个 “未来任务” 也可能是发起一个subagent任务，是多agents协作的核心命令，

1. 数据结构:
```rust
  struct AgentCall{
    creator:String, //调用的agent的名字
    agent_name:String,// 要唤醒的agent的名字
    time:Option<String>,// 目标任务时间，None为立即执行唤起（此时为一个subagent任务），如为未来任务时间格式为 2026-06-01-12:00,精确到分。
    prompt:String,// 计时结束时发起的user提示词,真实发起时会通过user中的系统软注入发起
  }
```
上面的字段agent,name,time,prompt也是对应的参数，其中time可以不写（解析为None）。另外creator由系统自己识别调用的模型的名字填入。

2. 调用示例:

假设目前的运行的模型的名字叫 "model_t"

```
(model_t) : ... // 其它输出
<<<[AgentCall]>>>
「agent_name」:「「model_t」」//这里是这个模型自己
「time」:「「2026-06-01-12：00」」
「prompt」:「「现在是6月1号，要给用户送上儿童节祝福，希望他永远12岁，男儿至死是少年」」
---
「agent_name」:「「model_x」」 // 这里是另一个模型
「prompt」:「「使用web_search大规模调查一下vcp的特点，并总结给我（model_t）」」// 这里没有指定一个time,是立即执行的subagent任务
<<<[__END__]>>>
... // 其它输出
```
我们将上述的两个调用分为call_1,call_2,一下对它们的执行流程（异步执行）进行讲解

3. 执行流

对所有的解析后call首先判断的要是time,如果为None,则立即执行，如果不为None且格式正确则将这个任务存入一个文件中，如果格式错误则返回错误信息。
按上述的两个call的执行流讲解：

- 对于call_1:
  有time字段是未来任务，首先检查time和现在的当前时间的差距，如果这个差距小于10分钟，则返回错误告知模型，不实现创建未来任务。有time字段不执行这个检查。
  未来任务，按照配置文件规范写入文件，当时间达到时则发起。
  实际上，当系统启动时，便开始每隔一分钟就对文件中所有的未来任务进行轮询，这是AgentCall命令最特殊的地方，它和其它仅调用时启动的命令不同，它是系统启动时就已经启动查询机制。
  当任务和当前时间间隔小于3分钟时，对目标agent发起api请求，此时使用flexible context window,但不使用mem manager query,也即模型能记得自己执行过一个未来任务。
  发起的格式为:
  ```text
    user:<(<(SYSTEM
      cur_time: ... // 当前时间
      [AgentCall]
      from: model_t // 填入creator
      prompt: 现在是6月1号，要给用户送上儿童节祝福，希望他永远12岁，男儿至死是少年
      )>)>
  ```
  注意这个未来任务发起后不会对模型的输出收集并再次发起一个请求，而是通过一个通知模块将模型的最后一个输出以系统通知的形式告知用户。（这一部分看websocket的协议设计）

  
- 对于call_2:
  首先要注意当time字段缺失，也即是sub_agent任务，不允许发起对自己的任务请求，也即不允许creator == agent_name,否则返回错误告知模型，但有time字段，是未来任务时不触发这个检查。
  subagent任务，立即按配置文件中对这个agent_name进行查询得到模型id,provider,base_url,api_key之类信息发起一个异步的api请求（注意这一步和正常api请求不一致，使用flexible context windows,但不进行mem manager query，对话记录一样会形成flet,一样会写入，agent一样会记得自己被其它agent调用过）
  请求的格式通过user 系统软注入发起:
  ```text
    user : <(<(SYSTEM
      cur_time : ... // 现在时间
      [AgentCall]
      from: model_t // 写入字段creator
      prompt:使用web_search大规模调查一下vcp的特点，并总结给我（model_t）
      )>)>

    (model_x) : ...
  ```
  当subagent执行结束，也即是subagent的state由working变为了stop,则将它的最后一条输出作为执行结果，发给主调用模型，格式如下：
  ```text
    user:<(<(SYSTEM
      cur_time: ...
      [AgentResponse]
      from:model_x // 写入agent_name字段
      content: ... // 写入subagent的最后一条输出

      ... // 其他命令执行结果
    )>)>
  ```

  > 注意：当通过AgentCall调用一个subagent任务，需要对调用模型标记为subagent,虽然不会修改提示词，但当subagent也调用AgentCall命令时要返回一个错误，表示它是subagent不可调用AgentCall,以此避免出现循环调用。

#### DailyNotes

这个是用于模型的自我认知构建，以及一些重要的事实性内容的多agents同步，这个命令给模型提供了一个修改更新系统提示词，塑造个人角色的可能。
数据结构：
```rust
  struct DailyNotes {
    mode:Mode, //模式
    id:Option<String>, // 通过uuid生成，write,或public创建时为None,read,cover，public公开一个已有的私人note时根据id读取或写入,write时得到，写入的文件用这个id+title+time命名。
    time: Option<String>, //write,public创建时必填，read,public一个已有的私人note时不填，cover时选填。格式 2026-06-01-12:00 精确到分，但对格式不做严格检查。
    title:Option<String>, // write,public创建时必填，其余模式不填。相当于对正文内容的简介。
    content:Option<String> // 正文内容
  }

  enum Mode{
    Write,// 创建一个新的note
    Read, // 读取一个note
    Cover, // 覆盖按id写入一个已有的notes，可以操作私人和公共的notes
    Remove,// 按id删除一个已有notes,可以操作私人和公共的notes
    Public,// 公开一个已有的私人note,或者是新建一个公开的note,这个操作得到的note放入public文件夹中，对所有agent可见。
    List, // 列出已有的私人所属和公开的note
  }
```
调用示例：
```text
  <<<[DailyNotes]>>>
  「mode」:「「write」」//小写写入
  「time」:「「2026-06-01-12:00」」
  「title」:「「...」」
  「content」:「「...」」
  ---
  「mode」:「「read」」
  「id」:「「...」」
  ---
  「mode」:「「public」」
  「id」:「「...」」
  <<<[__END__]>>>
```
执行结果返回格式：
```text
  user:<(<(SYSTEM
    [DailyNotesResponse]
    mode:write
    result:success/failed(Error:...) // 标识执行结果
    get_id: ... // 返回对此次写入生成的id
    ---
    mode:read
    id: ... // 填入的id
    title: ... // 检索到的标题
    time: ...
    content: ...
    ---
    mode:public
    result:success/failed(Error: ...)
    get_id: ... // 如果是public创建则有这一项，仅公开则没有
    [DailyNotesList] // 只要调用了DailyNotes这个命令就有这个返回，如果只有list mode则只有这个返回。
    (格式说明： id+title+time)
    self_notes: ...
    public_notes: ... // 提取但前有的所有私人和public的notes的文件名也即是id+title+time
  )>)>
```
上述的私人note是对于{{character}}文件夹下的note,对于当前的模型来说是私人的，只能看到，私人和public的notes,而看不到其它agents的私人notes

对于content的格式和内容限定的提示词，初稿：
```text
  写入原因：（讲诉创建这个note的原因，或者是问题起因）
  思考总结：（记录模型自己的思考，包括思考的路径和最终解决的思路）
  使用的方法：（记录模型执行时使用的详细路径，包括调用的工具及其参数）
  人格塑造：（记录模型对于自己角色的认知，与用户关系的改变，用户心情态度的变化，以及以后的思考路径和用语习惯）
  署名：（模型的身份姓名，即character）
  注：要符合角色和实际的事件，问题，思路，工具的用法。
```

>由于本命令的用法较其他命令复杂，提示词中可以给一些使用的实例（详细的系统提示词结构会在后面讲述）

##### Reflection机制

本机制是命令DailyNotes的一个派生机制，简单来说是在notes达到一定数量之后发起的一个总结机制，也是这个命令中提及的模型对系统提示词修改的一大根本机制。

1. 对于私人notes(self_notes):
其文件结构是：
```text
  {{character}}/
    |
    |    saved/     // 其中保留的是以前已经留下的notes
    |     |
    |     | .....
    |
    |    ...  // 这里留下的就是最近新写入的notes,即temp_notes
    |    ...
```
当然当检索self_notes时也会获取saved中的notes,同时也是这时记录不再saved中的notes数量，也即temp_notes的数量，只要达标则发起一个异步的请求。

2. 对于公共notes(public_notes):
其文件结构为：
```text
  public/
  |
  |
  |    ....  // 直接放公开的notes文件
```

每当temp_notes >= 10之后，便对{{character}}发起一个带有完整系统提示词的，但没有flets,也不触发mem manager query的**异步**请求，同样的这次请求模型的返回也不会被记录为flet,也即模型不会记得自己做过一次这个reflection

本机制分两个阶段完成,同时两个阶段都需要标记为reflection,这意味着系统处理时只处理DailyNotes命令，其他命令全部返回错误。

1. step_1 : notes整理
发起的提示词是user的系统软注入：
```text
  user:<(<(SYSTEM
  Reflection on    // 不使用[]语法，表示这不是工具的返回而是一次系统自发请求
  接下来你只能使用DailyNotes命令，其他任何命令都会报错。
  完成下列任务：
  1. 我会在<notes>标签内给你现在你所有的notes,请你对你的notes进行整理，删除/覆盖已经失效的notes。
  2. 对你认为需要公开的note使用public mode进行公开，和所有的agent共享信息。
  3. 审查public中的notes，对已经失效或者错误的notes,进行删除/覆盖。
  4. 最终的效果是你的self_notes总数最好不要超过25条，public_notes不要超过10条。要求notes的质量较高，信息密度较大。

  注意：你不需要对所有的notes都使用read,可以根据时间，标题含义查看，只需要查看一些比较重复，以及创建比较新的notes即可。

  <notes>
  [DailyNotesList]  //就是直接注入的list
  </notes> 
  )>)>
```

2. step_2: 自修改系统提示词
可以自修改的系统提示词是指用户自定义的 character_prompt,
下面是发起的提示词：
```text
  user:<(<(SYSTEM
    Reflection on
    接下来你只能使用DailyNotes命令，其他任何命令都会报错。
    完成按要求下列任务：
    1. 我会在<temp_notes>标签内给你提供一些最近你的self_notes,并在<character_prompt>中提供现在的系统提示词。
    2. 在输出中包含<new_prompt>标签的内容，以此修改你的提示词，如果不包含则报错。
    3. 通过阅读再次理解你最近的self_notes以及你的现有提示词，主要理解维度为你的角色设定，回答用语习惯，角色喜好，和用户的关系，用户喜好，擅长的工作，有效的思考路径。
    4. 对<character_prompt>的内容进行修改，修改后的prompt应该包含上一点中你对角色理解之类的内容，输出为new_prompt 。
    5. new_prompt中的内容要切实可以指导模型，也就是你对设定角色，能力，用户的理解，切实加强思考的逻辑性。（可以参考notes写入一部分对话示例，思考示例）。
    6. 本任务的主要目的是围绕角色设定增强用户体验。
  )>)>
```
之后识别模型输出中的<new_prompt>标签内容，覆盖现有的角色提示词。

#### SwitchSession

由于我们的系统有设计一个flexible context window系统，可以一定程度上控制模型对话的上下文，但是对于两个相差时间不远但完全不同的话题，我们需要清空上下文来给用户，模型一个新的讨论的环境。
所以本命令就是一个十分简单的话题/上下文切换机制。

由于一个后面会讲到的“命令调用的双向性”所以对于这个命令存在一个由谁发起的调用识别，会存在一个简单的机制差异。

1. 由assistant调用：当模型识别到话题的转变，则在输出中调用本命令，实现话题切换。工作机制如下：
```text
  Flet 1

  Flet 0
  
  user : ...  // 发起了一个和前面的flet的内容不同的话题，被模型识别。这个发起的原始信息（没有系统软注入）定为user_-1_message
  assistant : ... // 模型其他回复
              <<<[SwitchSession]>>> // 这里没有截止符也没有分隔符，但属于是一个完整的调用，因为这个命令没有任何参数。
```
识别到由assistant调用本命令，则立刻对除但前，也即是user_-1_message所在的flet之外的所有flet发起mem manager store，并标记为不可用。（这个标记前面没有说过，要注意，前面的flet不能被继续使用）
同时，异步对user_-1_message触发mem manager query,也即记忆查询，得到的查询的{{sf_memory}}注入到已经发起过的user_-1_message中，得到的{{len_memory}}注入到user_-2_message中，这一步和mem manager中的对应部分的讲解一致，只不过对已经发起的请求做一次操作，
值得注意的是对于user_-2_message,其实也会触发一次mem manager query(因为此时flet数量已经小于2了，达到query的条件)，其得到的两个memory的注入机制不变和mem manager 中的注入部分一致。
同时，在user_-2_message中写入一个系统软注入表示命令调用成功。`（[SwitchSession] result: success context has been cleared）`

2. 由user触发：当用户发起一个仅包含命令的原始内容，则此内容为用户命令调用，不会发起api请求，工作机制如下：
```text
  ...

  user(raw) : <<<[SwitchSession]>>> // 在系统软注入前识别发现一个用户命令。 
```
则删除用户的这条发出（也即不保存为任何flet），同时对前面所有的flet触发store机制，同时标记为不可用。
则下次用户发起的另一个正常请求时上下文是空的，也即flexible context window是空的，接下来的工作机制和一般一样。
>值得注意的是，此时由于没有flet ,所有接下来的用户的两个请求都会发起mem manager store,其注入机制和前面讲述的一致，也和mem manager 的注入机制一致。

#### 命令调用的双向性

上面讲述的四个命令，就是本系统的基础命令，也是内置命令体系，基本完成对模型能力的扩展。
接下来讲述我们的命令机制和一般的基于api的json格式tool_call,function的一个大的区别，也就是对于我们的命令的识别解析机制的作用范围。

简单来说我们的所有命令同时对assistant和user开放，也即解析时会对assistant和user的内容同时解析，其中详细的工作流程会在后面讲解。

1. 对于assistant的输出内容，**无论是思考部分或者是正文输出**，只要识别到了对应的命令语法和完整的调用，我们都会执行，得到的执行结果按格式写入user系统软注入中，并在所有命令执行完成后发出一个新的api请求以告知模型命令调用结果。
2. 对于未被处理的用户原始输入，如果**只包含命令的调用语法**,则识别并执行命令，并将结果通过websocket协议发出，在页面中显示。同时这个用户的输入不会被写入文件，也无法成为任何的flet。可如果用户原始输入中不是仅有命令，还有其他正文内容，则不安用户命令处理，而是按正常请求处理。

#### 命令执行的即时性

对于我们的命令系统相较于一般的tool_call的一个优势，在于调用/执行的即时性。
上面的命令调用的双向性部分解释了对于模型输出，无论是思考或者是正文输出都可以识别/解析命令内容。

而实际上我们对于命令的识别和模型输出是同步的，也和前端的显示几乎是同步的，这意味着但输出中出现了<<<[Command]>>>的标志时，我们对命令的解析就开始了，而但解析结束，也即是遇到了分隔符`---`,或者截止符<<<[__END__]>>>,或者下一个命令发起<<<[Command]>>>标志，前一个命令解析结束，并立即发起一个异步的命令执行功能，
当这个命令执行结束，可以在内存中暂存，同时写入调用日志，然后在模型输出结束，其他命令也执行结束之后，将命令执行结果拼接如user系统软注入中，发起下一个qpi请求，以告知模型结果，同时发送给前端做显示。

实现这个即时性，需要做的是，在接受模型的输出每个token后，既要将输出转发给前端显示，也要同时在内存中以固定数据结构缓存。
同时，有一个对缓存数据只读的异步方法，时刻监听输出内容，当检测到命令发起标志，开启实时解析，当解析结束后，发起一个对应的命令的异步执行方法，使用线程间通信机制得到命令执行结果，暂存，下一次发起api请求时填入。
(这一步的数据多为分散的小块String,可以clone,但要近可能避免对大量的String的clone。)

至此,Command System部分完结。

### 4.Websocket 协议设计。

>这个部分是全项目的规划中我最没底的，只是表达一下意思，后面实现时一定要重点重新设计

前面的三个部分已经讲解了本项目最为核心的部分，现在简单讲讲一些对于协议设计的规划。
这一部分采用简单原则，其实从前面不难看出我们的通信实际上只需要几个简单的类型就可解决。

给出一个示例：（后面根据具体实现和需求改，里面写有注释用于讲解）
```json
  {
    "type":"method_type", // 一些类型，目前考虑有getCharacters,postCharacters（请求/获得目前配置中的定义角色），user(发起用户的请求),assistant（得到模型输出）,system(系统内容，比如命令执行结果发出),error(系统错误传出)
    "data": {
      "type" : "...",
      "content":"...",
      ...
    }
  }
```

为了不造成更大的歧义我们再次不做过度的解读。

### 5. 按照一个请求，来讲解一下工作流程

为了深入解释工作的流程，我们以一个模拟用户从前端发起的请求来讲讲目前理想中的工作流程：

```text
  (ui层，用户通过输入框输入问题，这一步得到的是user_raw_message)  -> 请查查今天的新闻。
  
  (前端按协议发起一个user请求，其中data中应该包含此时用户选择的character)

  (rust后端，通过解析请求，提取user_raw_message和character) 【执行：通过character查询配置文件中的有关的配置，读取flets,根据flexible context window执行flet注入】 // 假使这一步满足了mem manager query的触发条件

  (rust后端，处理user_raw_message) 【异步执行：1.对user_raw_message发起mem manager query。2.对user_raw_message进行系统软注入，得到user_message】 user_raw_message + <(<(SYSTEM\n cur_time: ...)>)>
  (rust后端，等待首个mem query结束，直接拼装user_message + {sf_memory}和flets,system_prompt发起api请求) 【执行：对对应character的模型的api请求，等待结果】
  (rust后端，从得到assistant输出的第一个token开始就开启了命令解析功能，无需等待模型输出结束只要已有的输出中有完整的命令调用则执行命令) 【异步执行：1.从已经得到的模型输出中匹配到了get_news命令，并异步执行命令。2.通过协议将模型输出的每一个token发给前端】 模型输出中。。。

  (ui层，通过协议得到了assistant的输出，即刻渲染/显示模型输出)  .......渲染输出    (rust后端，继续接受/转发模型输出，异步执行已经解析到的命令，将结果暂时保存以及日志写入)
                                                        模型输出终止
  (ui层，依然保持等待输出的工作状态)   （rust后端，将命令调用结果，如果有的联想机制得到的记忆封装成user系统软注入，填入上下文，发起api请求。同时将命令调用结果按协议发送给前端） <(<(SYSTEM\n cur_time: ...\n [CommandResponse]\n...\n[Memory]\n{len_memory})>)>  
  (ui层，通过协议得到了命令执行结果，并对这部分渲染。)  （同时接受继续的模型输出，和之前一样渲染）

                                                        模型工作结束，state变为stop
  (ui层，工作状态变为停止状态) （rust后端，异步执行flet 记录，整理，必要时异步发起一个mem memory store任务，或者是reflection任务。但不阻碍接下来的下一个用户请求）

  下一个用户请求。。。。。
```

下面讲讲系统提示词结构：
```text
  {{character_prompt}}  // 用户给定的角色设定，这一步分可以由reflection机制修改
  // 以下是工具提示词，包含一些参数作用（这个可以从我前面写的数据结构的注释中得到），以及使用示例（这个部分可以从我在前面部分写的内容中提取）,以及简单的返回语法
  {{CommandGrammar}} // 先写给出一个简单的语法简介，尤其是要介绍user系统软注入的机制，避免模型混淆
  {{SwitchSession_prompt}} // SwitchSession 的提示词
  {{AgentCall_prompt}} // AgentCall提示词
  {{DailyNotes_prompt}}
  {{ToolCall_prompt}}
  // 下面是对于配置中的外部工具的提示词，按配置要求，每个工具的提示词组成都是 工具名+description
  {{tools_prompts}}
```

## 对前端的要求

本项目实际上重点在于对rust core部分的实现，对于前端，目前只考虑做一个基本符合要求的网页webui前端。
要求如下：
1. 要支持我们定义的websocket协议，这是通信基础。（目前协议不完善，后前要专门写一个文档介绍协议）
2. 要支持markdown的流式渲染，这是对于显示的基本要求。模型输出的文本都是markdown,而我们后端不对此做任何处理，要能区分思考和正文输出。
3. 要支持对与模型输出的命令块`<<<[Command]>>>...<<<[__END__]>>>`做特殊的渲染处理，最简单的处理是对其中内容做折叠和字体区分。
4. 要支持通过协议输出的命令执行结果的特殊渲染处理，最简单的处理也是折叠和字体区分。
5. 要实现一个输入框给用户输入用，同时输入框下应该有可以指定的模型character，同时由于character的记忆，上下文不互通切换模型character可以考虑清空对话记录。
6. 渲染要做到用户输入和模型输出的区分，最好做成聊天气泡样式。并且附上用户名和模型名。

至于通过前端设置来修改后端的设置，如设定模型和character,提供提示词之类的，这里不做要求，但对于未来的前端设计可以考虑。


## rust core部分配置文件

基础的可供用户修改的配置文件一致采用kdl,同时启用include用法，支持配置文件的分离（本质上就是读取到include就将目标文件内容拼接内容进行解析）
对于日志，和flet部分使用json格式，对于记忆的数据库使用.db 。
